//! Server state: database, repository, transport (client websockets and
//! peer links), identity, root and registry view, derived access and trust,
//! login limiter, peering, and the open sync sessions. [`App`] is also what the admin commands run against, either
//! inside `serve` (through the admin socket) or directly on the database
//! when no server holds it.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use automerge_repo::{
    DocHandle, DocumentId, DocumentStatus, Error as RepoError, Repo, RepoConfig, SqliteStorage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{Notify, watch},
    task::JoinHandle,
};
use tracing::{info, warn};
use tt_core::{model::User, schema};

use crate::{
    access::{Access, AccessIndex, DerivedPolicy, Member},
    admin::{AdminError, DbLock, check_name, complete_reset},
    admin_socket,
    auth::RateLimiter,
    db::{Db, JoinIntent, RootRecord, TokenRecord, now_ms},
    identity::{Identity, host_name, key_path},
    peer::Peering,
    registry::{self, Account, NewAccount, NewServer, RegistryView},
    transport::ServerTransport,
};

/// How long `serve` waits for a database lock held by a direct admin
/// command before giving up.
const LOCK_GRACE: Duration = Duration::from_secs(2);

/// Logged and answered while the server has no root.
pub const NOT_SET_UP: &str = "this server is not set up: run `tt-server init --name <name>` to create its root, or `tt-server peer join` to join an existing server";

#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// `server.db`: root record, tokens, and documents.
    pub db: PathBuf,
    /// Built web bundle served at `/` with SPA fallback.
    pub web_dir: Option<PathBuf>,
    /// Idle documents are flushed and dropped from memory after this long.
    pub idle_eviction: Duration,
    /// Lifetime of a websocket ticket.
    pub ticket_ttl: Duration,
    /// How long a pushed, unknown document may wait for the user's index to
    /// list it before the connection is closed.
    pub pending_timeout: Duration,
    /// How often open sessions are checked for tokens revoked by direct
    /// database writes (admin commands run while no server held the lock).
    pub revocation_poll: Duration,
    /// Take the client address from `X-Forwarded-For`/`X-Real-IP` (only
    /// behind a trusted reverse proxy).
    pub behind_proxy: bool,
    /// Login attempts allowed per IP per `login_window`.
    pub login_limit: usize,
    pub login_window: Duration,
    /// Delay before redialing a peer after a failed attempt or a drop.
    pub peer_retry: Duration,
    /// How long `peer join` waits for the root to be fetched.
    pub join_timeout: Duration,
}

impl ServerOptions {
    #[must_use]
    pub fn new(db: impl Into<PathBuf>) -> Self {
        Self {
            db: db.into(),
            web_dir: None,
            idle_eviction: Duration::from_secs(600),
            ticket_ttl: Duration::from_secs(60),
            pending_timeout: Duration::from_secs(10),
            revocation_poll: Duration::from_secs(5),
            behind_proxy: false,
            login_limit: 5,
            login_window: Duration::from_secs(60),
            peer_retry: Duration::from_secs(3),
            join_timeout: Duration::from_secs(30 * 60),
        }
    }
}

/// Whether the server has a root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RootState {
    NeedsDecision,
    /// Fetching another server's root (`peer join`); clients are refused.
    Joining,
    Ready,
}

/// How an admin command names an account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRef {
    /// The account owning the name.
    Name(String),
    Id(String),
}

impl std::fmt::Display for UserRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(name) => write!(f, "{name}"),
            Self::Id(id) => write!(f, "id {id}"),
        }
    }
}

pub(crate) struct Session {
    pub token_hash: String,
    pub user_id: String,
    pub close: Arc<Notify>,
}

struct Root {
    record: RootRecord,
    registry: DocHandle,
}

pub struct App {
    pub(crate) options: ServerOptions,
    pub(crate) db: Db,
    pub(crate) repo: Repo,
    pub(crate) transport: Arc<ServerTransport>,
    pub(crate) access: Arc<AccessIndex>,
    pub(crate) limiter: RateLimiter,
    pub(crate) peering: Peering,
    identity: Identity,
    root: RwLock<Option<Root>>,
    /// Set while fetching another server's root.
    joining: RwLock<Option<JoinIntent>>,
    state: watch::Sender<RootState>,
    view: RwLock<Arc<RegistryView>>,
    /// Serializes registry writes and `init`.
    pub(crate) writer: tokio::sync::Mutex<()>,
    /// Serializes view rebuilds.
    refreshing: tokio::sync::Mutex<()>,
    sessions: Mutex<HashMap<u64, Session>>,
    next_session: AtomicU64,
    pub(crate) tasks: Mutex<Vec<JoinHandle<()>>>,
}

fn change_error(error: automerge::AutomergeError) -> RepoError {
    RepoError::Change(error.to_string())
}

fn check_hash(hash: &str) -> Result<()> {
    if !hash.starts_with("$argon2id$") {
        return Err(
            AdminError::Invalid("password hashes must be argon2id PHC strings".into()).into(),
        );
    }
    Ok(())
}

pub(crate) fn check_server_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(
            AdminError::Invalid("server names are 1-64 printable characters".into()).into(),
        );
    }
    Ok(())
}

impl App {
    /// Opens the database (refusing old formats), completes an interrupted
    /// reset, opens the identity and the repository (documents load
    /// lazily), and reads the root if there is one, or resumes a join. The
    /// caller holds the database lock.
    pub(crate) async fn open(options: ServerOptions) -> Result<Arc<Self>> {
        let db = Db::open(&options.db)?;
        if complete_reset(&db, &key_path(&options.db))? {
            info!("completed an interrupted reset");
        }
        let identity = Identity::load_or_create(&key_path(&options.db))?;
        let storage = Arc::new(
            SqliteStorage::open(&options.db)
                .with_context(|| format!("opening {}", options.db.display()))?,
        );
        let access = AccessIndex::new();
        let transport = ServerTransport::new(identity.server_id());
        let repo = Repo::open_with_policy(
            storage.clone(),
            storage,
            transport.clone(),
            RepoConfig {
                lazy_load: true,
                idle_eviction: Some(options.idle_eviction),
                ..RepoConfig::default()
            },
            Arc::new(DerivedPolicy(access.clone())),
        )
        .await?;
        let root = match db.root()? {
            Some(record) => {
                let id = DocumentId::parse_any(&record.registry_doc)?;
                let registry = match repo.open_document(id).await {
                    Ok(handle) if handle.status() == DocumentStatus::Ready => handle,
                    _ => {
                        return Err(anyhow!(
                            "the registry document {} is missing from {}",
                            record.registry_doc,
                            options.db.display()
                        ));
                    }
                };
                Some(Root { record, registry })
            }
            None => None,
        };
        let joining = if root.is_none() { db.joining()? } else { None };
        let peering = Peering::new(&identity, &access)?;
        let app = Arc::new(Self {
            limiter: RateLimiter::new(options.login_limit, options.login_window),
            options,
            db,
            repo,
            transport,
            access,
            peering,
            identity,
            root: RwLock::new(None),
            joining: RwLock::new(None),
            state: watch::Sender::new(RootState::NeedsDecision),
            view: RwLock::new(Arc::default()),
            writer: tokio::sync::Mutex::new(()),
            refreshing: tokio::sync::Mutex::new(()),
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicU64::new(1),
            tasks: Mutex::new(Vec::new()),
        });
        app.watch_inventory();
        if let Some(root) = root {
            app.install_root(root).await?;
        } else if let Some(intent) = joining {
            info!("resuming an interrupted join");
            app.resume_join(intent).await?;
        }
        Ok(app)
    }

    fn publish_state(&self) {
        self.state.send_replace(self.state());
    }

    /// Watches the root state.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<RootState> {
        self.state.subscribe()
    }

    async fn install_root(self: &Arc<Self>, root: Root) -> Result<()> {
        let handle = root.registry.clone();
        self.access.set_registry(Some(handle.id()));
        *self.root.write().unwrap() = Some(root);
        self.publish_state();
        self.watch_registry(handle);
        self.refresh().await
    }

    /// Installs the registry being fetched: the closure and trust follow it
    /// as it arrives, but the server stays `Joining`.
    pub(crate) async fn install_joining(
        self: &Arc<Self>,
        intent: JoinIntent,
        registry: DocHandle,
    ) -> Result<()> {
        let record = RootRecord {
            server_id: self.identity.server_id().to_owned(),
            name: intent.name.clone(),
            registry_doc: intent.registry_doc.clone(),
            created: now_ms(),
        };
        *self.joining.write().unwrap() = Some(intent);
        self.install_root(Root { record, registry }).await
    }

    /// Records the fetched root: from now on the server is `Ready`.
    pub(crate) async fn finish_join(&self) -> Result<()> {
        self.repo.flush().await?;
        let Some(intent) = self.joining.read().unwrap().clone() else {
            return Ok(());
        };
        let record = RootRecord {
            server_id: self.identity.server_id().to_owned(),
            name: intent.name.clone(),
            registry_doc: intent.registry_doc.clone(),
            created: now_ms(),
        };
        let row = record.clone();
        self.db.run(move |db| db.finish_join(&row)).await?;
        if let Some(root) = self.root.write().unwrap().as_mut() {
            root.record = record;
        }
        *self.joining.write().unwrap() = None;
        self.access.set_joining(None);
        self.refresh().await?;
        self.publish_state();
        info!(name = %intent.name, registry = %intent.registry_doc, "joined; the root is stored");
        Ok(())
    }

    /// Rebuilds the view on every registry change, local or merged.
    fn watch_registry(self: &Arc<Self>, handle: DocHandle) {
        let mut events = handle.subscribe();
        let app = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match events.recv().await {
                    Ok(_) | Err(RecvError::Lagged(_)) => {
                        let Some(app) = app.upgrade() else { return };
                        if let Err(error) = app.refresh().await {
                            warn!(error = %format!("{error:#}"), "rebuilding the registry view failed");
                        }
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        });
        self.tasks.lock().unwrap().push(task);
    }

    /// Re-reads the registry: revokes tokens and closes sessions of newly
    /// deleted accounts, and updates the access index.
    pub(crate) async fn refresh(&self) -> Result<()> {
        let _refreshing = self.refreshing.lock().await;
        let Some(handle) = self.registry_handle() else {
            return Ok(());
        };
        let view = Arc::new(handle.read(registry::read).await?);
        let previous = std::mem::replace(&mut *self.view.write().unwrap(), view.clone());
        for account in view.accounts() {
            let newly_deleted = account.is_deleted() && previous.active(&account.id).is_some()
                || account.is_deleted() && previous.get(&account.id).is_none();
            if newly_deleted {
                self.revoke_account(account).await?;
            }
        }
        // Deleted accounts own nothing, but stay in the registry closure.
        let members = view
            .accounts()
            .into_iter()
            .filter_map(|account| {
                Some(Member {
                    id: account.id.clone(),
                    created: account.created,
                    index: DocumentId::parse_any(&account.index_doc).ok()?,
                    active: !account.is_deleted(),
                })
            })
            .collect();
        for member in self.access.set_members(members) {
            let listing = self.index_listing(member.index).await?;
            self.access.set_listing(&member.id, listing);
        }
        // Trust: non-revoked members other than this server. Links to
        // servers no longer trusted close at once.
        let own = self.identity.server_id();
        let servers = view
            .servers()
            .into_iter()
            .filter(|entry| !entry.is_revoked() && entry.id != own)
            .filter_map(|entry| Some((entry.id.clone(), entry.public_key()?)))
            .collect();
        self.access.set_servers(servers);
        for server in self.transport.linked() {
            if !self.access.is_trusted_server(&server) {
                info!(server = %crate::identity::id_prefix(&server), "member no longer trusted; closing its link");
                self.transport.close_link(&server);
            }
        }
        Ok(())
    }

    async fn revoke_account(&self, account: &Account) -> Result<()> {
        let user_id = account.id.clone();
        let hashes = self
            .db
            .run(move |db| db.revoke_user_tokens(&user_id))
            .await?;
        let mut closed = 0;
        for hash in &hashes {
            closed += self.close_token_sessions(hash);
        }
        closed += self.close_user_sessions(&account.id);
        if !hashes.is_empty() || closed > 0 {
            info!(user = %account.name, tokens = hashes.len(), closed, "account deleted; revoked its tokens");
        }
        Ok(())
    }

    pub(crate) fn registry_handle(&self) -> Option<DocHandle> {
        self.root
            .read()
            .unwrap()
            .as_ref()
            .map(|root| root.registry.clone())
    }

    /// The registry of a `Ready` server.
    pub(crate) fn registry(&self) -> Result<DocHandle> {
        if self.joining.read().unwrap().is_some() {
            return Err(AdminError::Joining.into());
        }
        self.registry_handle()
            .ok_or_else(|| AdminError::NotSetUp.into())
    }

    #[must_use]
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    #[must_use]
    pub fn state(&self) -> RootState {
        if self.joining.read().unwrap().is_some() {
            RootState::Joining
        } else if self.root.read().unwrap().is_some() {
            RootState::Ready
        } else {
            RootState::NeedsDecision
        }
    }

    #[must_use]
    pub fn root(&self) -> Option<RootRecord> {
        self.root
            .read()
            .unwrap()
            .as_ref()
            .map(|root| root.record.clone())
    }

    /// `{id, name}` as the API reports it; `name` is null before `init`.
    #[must_use]
    pub fn server_json(&self) -> Value {
        json!({
            "id": self.identity.server_id(),
            "name": self.root().filter(|_| self.state() == RootState::Ready).map(|root| root.name),
        })
    }

    /// The current registry view.
    #[must_use]
    pub fn view(&self) -> Arc<RegistryView> {
        self.view.read().unwrap().clone()
    }

    // ---------- sessions ----------

    pub(crate) fn register(&self, session: Session) -> u64 {
        let id = self.next_session.fetch_add(1, Ordering::Relaxed);
        self.sessions.lock().unwrap().insert(id, session);
        id
    }

    pub(crate) fn unregister(&self, id: u64) {
        self.sessions.lock().unwrap().remove(&id);
    }

    /// Closes every open session authenticated by `token_hash`.
    pub(crate) fn close_token_sessions(&self, token_hash: &str) -> usize {
        self.close_sessions(|session| session.token_hash == token_hash)
    }

    /// Closes every open session of a user.
    pub(crate) fn close_user_sessions(&self, user_id: &str) -> usize {
        self.close_sessions(|session| session.user_id == user_id)
    }

    fn close_sessions(&self, matches: impl Fn(&Session) -> bool) -> usize {
        let sessions = self.sessions.lock().unwrap();
        let mut closed = 0;
        for session in sessions.values().filter(|s| matches(s)) {
            session.close.notify_one();
            closed += 1;
        }
        closed
    }

    /// Ends every sync session (shutdown).
    pub async fn close_all_sessions(&self) {
        use automerge_repo::network::NetworkTransport;
        for session in self.sessions.lock().unwrap().values() {
            session.close.notify_one();
        }
        let _ = self.transport.close().await;
    }

    /// The repository peer id this server announces (`srv:<server id>`).
    #[must_use]
    pub fn peer_id(&self) -> automerge_repo::PeerId {
        use automerge_repo::network::NetworkTransport;
        self.transport.local_peer()
    }

    /// Number of open sync sessions (for tests and health).
    #[must_use]
    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    /// Open sessions of one user.
    #[must_use]
    pub fn user_sessions(&self, user_id: &str) -> usize {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.user_id == user_id)
            .count()
    }

    // ---------- documents and access ----------

    /// A stored document, or `None` when the server has never stored it.
    /// Never creates a placeholder or asks peers.
    pub(crate) async fn stored(&self, id: DocumentId) -> Result<Option<DocHandle>> {
        match self.repo.open_document(id).await {
            Ok(handle) if handle.status() == DocumentStatus::Ready => Ok(Some(handle)),
            Ok(_) | Err(RepoError::NotFound(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Document ids an index lists (workspace and entries), as the server
    /// currently holds the index.
    pub(crate) async fn index_listing(&self, index: DocumentId) -> Result<Vec<DocumentId>> {
        let Some(handle) = self.stored(index).await? else {
            return Ok(Vec::new());
        };
        let view = handle.read(tt_core::schema::read_index).await?;
        Ok(view
            .workspace
            .iter()
            .chain(view.entries.values())
            .filter_map(|id| DocumentId::parse_any(id).ok())
            .collect())
    }

    /// The user owning a document, as derived from the indexes.
    #[must_use]
    pub fn owner_of(&self, doc: DocumentId) -> Option<String> {
        self.access.owner(doc)
    }

    /// Re-reads a user's index into the access index.
    pub(crate) async fn refresh_listing(&self, user_id: &str) -> Result<()> {
        if let Some(member) = self.access.member(user_id) {
            let listing = self.index_listing(member.index).await?;
            self.access.set_listing(user_id, listing);
        }
        Ok(())
    }

    /// Access from memory; anything but `Granted` re-reads the user's index
    /// first, so documents it lists since the last read are seen.
    pub(crate) async fn access_for(&self, doc: DocumentId, user_id: &str) -> Result<Access> {
        let access = self.access.access(doc, user_id);
        if access == Access::Granted {
            return Ok(access);
        }
        self.refresh_listing(user_id).await?;
        Ok(self.access.access(doc, user_id))
    }

    // ---------- administration ----------

    /// Creates the root: the registry document, then the `server` row. A
    /// crash in between leaves an unreferenced document that is ignored.
    pub async fn init(self: &Arc<Self>, name: Option<String>) -> Result<RootRecord> {
        let _writer = self.writer.lock().await;
        if self.joining.read().unwrap().is_some() {
            return Err(AdminError::Joining.into());
        }
        if self.root.read().unwrap().is_some() {
            return Err(AdminError::AlreadyInitialized.into());
        }
        let name = name.unwrap_or_else(host_name);
        check_server_name(&name)?;
        // This server is the first member.
        let first = NewServer {
            id: self.identity.server_id().to_owned(),
            name: name.clone(),
            pubkey: self.identity.public_key().to_vec(),
            added_by: self.identity.server_id().to_owned(),
            added_at: now_ms(),
        };
        let registry = self
            .repo
            .create_with(move |tx| {
                registry::init(tx)
                    .and_then(|()| registry::add_server(tx, &first))
                    .map_err(change_error)
            })
            .await?;
        self.repo.flush().await?;
        let record = RootRecord {
            server_id: self.identity.server_id().to_owned(),
            name,
            registry_doc: registry.id().to_bs58check(),
            created: now_ms(),
        };
        let row = record.clone();
        if !self.db.run(move |db| db.insert_root(&row)).await? {
            return Err(AdminError::AlreadyInitialized.into());
        }
        info!(name = %record.name, server_id = %record.server_id, "root created");
        self.install_root(Root {
            record: record.clone(),
            registry,
        })
        .await?;
        Ok(record)
    }

    fn resolve(&self, user: &UserRef) -> Result<Account> {
        let view = self.view();
        let account = match user {
            UserRef::Name(name) => view.owner(name),
            UserRef::Id(id) => view.active(id),
        };
        account
            .cloned()
            .ok_or_else(|| AdminError::NoSuchUser(user.to_string()).into())
    }

    /// Applies one registry change and waits until the view reflects it.
    pub(crate) async fn write_registry(
        &self,
        change: impl FnOnce(
            &mut automerge::transaction::Transaction<'_>,
        ) -> Result<(), automerge::AutomergeError>
        + Send
        + 'static,
    ) -> Result<()> {
        self.registry()?
            .change(move |tx| change(tx).map_err(change_error))
            .await?;
        self.repo.flush().await?;
        self.refresh().await
    }

    /// Creates the account and its index and workspace documents. Fails with
    /// [`AdminError::Duplicate`] (and changes nothing) when a non-deleted
    /// account has the name.
    pub async fn add_user(&self, name: &str, password_hash: &str) -> Result<Account> {
        check_name(name)?;
        check_hash(password_hash)?;
        let _writer = self.writer.lock().await;
        self.registry()?;
        if self.view().owner(name).is_some() {
            return Err(AdminError::Duplicate(name.into()).into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let user = User {
            id: id.clone(),
            name: name.to_owned(),
        };
        let workspace = self
            .repo
            .create_with(move |tx| schema::init_workspace(tx, &user).map_err(change_error))
            .await?;
        let workspace_doc = workspace.id().to_bs58check();
        let listed = workspace_doc.clone();
        let index = self
            .repo
            .create_with(move |tx| schema::init_index(tx, &listed).map_err(change_error))
            .await?;
        let account = NewAccount {
            id: id.clone(),
            name: name.to_owned(),
            index_doc: index.id().to_bs58check(),
            workspace_doc,
            password_hash: password_hash.to_owned(),
            created: now_ms(),
        };
        self.write_registry(move |tx| registry::add_account(tx, &account))
            .await?;
        self.view()
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("account {id} missing after add"))
    }

    pub async fn set_password(&self, user: &UserRef, password_hash: &str) -> Result<Account> {
        check_hash(password_hash)?;
        let _writer = self.writer.lock().await;
        let account = self.resolve(user)?;
        let at = now_ms().max(account.password_changed_at + 1);
        let (id, hash) = (account.id.clone(), password_hash.to_owned());
        self.write_registry(move |tx| registry::set_password(tx, &id, &hash, at))
            .await?;
        Ok(account)
    }

    pub async fn rename_user(&self, user: &UserRef, new_name: &str) -> Result<Account> {
        check_name(new_name)?;
        let _writer = self.writer.lock().await;
        let account = self.resolve(user)?;
        if self
            .view()
            .owner(new_name)
            .is_some_and(|owner| owner.id != account.id)
        {
            return Err(AdminError::Duplicate(new_name.into()).into());
        }
        let at = now_ms().max(account.name_changed_at + 1);
        let (id, name) = (account.id.clone(), new_name.to_owned());
        self.write_registry(move |tx| registry::set_name(tx, &id, &name, at))
            .await?;
        Ok(self.view().get(&account.id).cloned().unwrap_or(account))
    }

    /// Tombstones the account: its tokens are revoked, its sessions closed,
    /// and its documents kept.
    pub async fn delete_user(&self, user: &UserRef) -> Result<Account> {
        let _writer = self.writer.lock().await;
        let account = self.resolve(user)?;
        let id = account.id.clone();
        self.write_registry(move |tx| registry::delete_account(tx, &id, now_ms()))
            .await?;
        Ok(self.view().get(&account.id).cloned().unwrap_or(account))
    }

    /// Every account, deleted and conflicted ones included.
    pub fn users(&self) -> Result<Vec<Account>> {
        self.registry()?;
        Ok(self.view().accounts().into_iter().cloned().collect())
    }

    /// Every token, with user names from the registry.
    pub async fn tokens(&self) -> Result<Vec<TokenRecord>> {
        let view = self.view();
        let mut tokens = self.db.run(|db| db.tokens()).await?;
        for token in &mut tokens {
            token.user_name = view
                .get(&token.user_id)
                .map_or_else(|| "?".into(), |account| account.name.clone());
        }
        tokens.sort_by(|a, b| (&a.user_name, a.created).cmp(&(&b.user_name, b.created)));
        Ok(tokens)
    }

    /// Revokes the live token whose id (hash prefix, at least 6 characters)
    /// matches and closes its sessions.
    pub async fn revoke_token(&self, id: &str) -> Result<TokenRecord> {
        if id.len() < 6 {
            return Err(
                AdminError::Invalid("give at least 6 characters of the token id".into()).into(),
            );
        }
        let matches: Vec<TokenRecord> = self
            .tokens()
            .await?
            .into_iter()
            .filter(|token| token.revoked.is_none() && token.token_hash.starts_with(id))
            .collect();
        let token = match matches.as_slice() {
            [] => return Err(AdminError::NoSuchToken(id.into()).into()),
            [one] => one.clone(),
            _ => return Err(AdminError::AmbiguousToken(id.into()).into()),
        };
        let hash = token.token_hash.clone();
        self.db
            .run(move |db| db.revoke_token(&hash))
            .await
            .context("revoking token")?;
        let closed = self.close_token_sessions(&token.token_hash);
        info!(token = token.id(), user = %token.user_name, closed, "token revoked");
        Ok(token)
    }

    /// Revokes every live token of a user and closes their sessions;
    /// returns how many tokens.
    pub async fn revoke_user_tokens(&self, user: &UserRef) -> Result<usize> {
        let account = self.resolve(user)?;
        let user_id = account.id.clone();
        let hashes = self
            .db
            .run(move |db| db.revoke_user_tokens(&user_id))
            .await?;
        for hash in &hashes {
            self.close_token_sessions(hash);
        }
        Ok(hashes.len())
    }

    // ---------- lifecycle ----------

    async fn poll_revocations(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.options.revocation_poll);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let hashes: Vec<String> = {
                let sessions = self.sessions.lock().unwrap();
                let mut hashes: Vec<String> =
                    sessions.values().map(|s| s.token_hash.clone()).collect();
                hashes.sort();
                hashes.dedup();
                hashes
            };
            if hashes.is_empty() {
                continue;
            }
            let dead = match self.db.run(move |db| db.dead_tokens(hashes)).await {
                Ok(dead) => dead,
                Err(error) => {
                    warn!(%error, "revocation poll failed");
                    continue;
                }
            };
            for hash in dead {
                let closed = self.close_token_sessions(&hash);
                if closed > 0 {
                    info!(
                        token = &hash[..12],
                        closed, "token revoked; closed sync sessions"
                    );
                }
            }
        }
    }

    /// Stops background tasks, closes sessions, and flushes and closes the
    /// repository.
    pub async fn close(&self) -> Result<()> {
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
        self.close_all_sessions().await;
        self.repo.clone().shutdown().await?;
        Ok(())
    }
}

/// A running server: state plus background tasks and the admin socket.
/// Bind it with [`Server::spawn_http`] (tests, reverse proxy) or through
/// `serve`.
pub struct Server {
    app: Arc<App>,
    tasks: Vec<JoinHandle<()>>,
    socket: PathBuf,
    lock: Option<DbLock>,
}

impl Server {
    /// Takes the database lock (failing when another process holds it),
    /// opens the database and repository, and starts the admin socket and
    /// the revocation poller.
    pub async fn open(options: ServerOptions) -> Result<Self> {
        // A direct admin command holds the lock briefly; wait for it.
        let deadline = std::time::Instant::now() + LOCK_GRACE;
        let lock = loop {
            if let Some(lock) = DbLock::try_acquire(&options.db)? {
                break lock;
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "{} is in use by another tt-server process (lock {} is held); stop it first",
                    options.db.display(),
                    crate::admin::lock_path(&options.db).display()
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let app = App::open(options).await?;
        let socket = admin_socket::socket_path(&app.options.db);
        let listener = admin_socket::bind(&socket)?;
        let tasks = vec![
            tokio::spawn(app.clone().poll_revocations()),
            tokio::spawn(admin_socket::serve(app.clone(), listener)),
        ];
        match app.state() {
            RootState::NeedsDecision => warn!("{NOT_SET_UP}"),
            RootState::Ready => app.register_self().await?,
            RootState::Joining => {}
        }
        app.dial_known().await?;
        Ok(Self {
            app,
            tasks,
            socket,
            lock: Some(lock),
        })
    }

    #[must_use]
    pub fn app(&self) -> &Arc<App> {
        &self.app
    }

    #[must_use]
    pub fn repo(&self) -> &Repo {
        &self.app.repo
    }

    pub fn router(&self) -> axum::Router {
        crate::api::router(self.app.clone())
    }

    /// Accepts peer links and pairings on `listener` in the background and
    /// advertises its address in invite codes (the host name when it binds
    /// every interface).
    pub fn spawn_peer_listener(&mut self, listener: TcpListener) -> SocketAddr {
        let address = listener
            .local_addr()
            .expect("bound listener has an address");
        let advertised = if address.ip().is_unspecified() {
            format!("{}:{}", host_name(), address.port())
        } else {
            address.to_string()
        };
        self.app.set_peer_address(advertised);
        self.tasks
            .push(tokio::spawn(self.app.clone().serve_peers(listener)));
        address
    }

    /// Keeps a link to the member at `addr` (a `--peer` seed).
    pub fn add_peer(&self, addr: impl Into<String>) {
        self.app.dial(addr.into());
    }

    /// Serves plain HTTP on `listener` in the background.
    pub fn spawn_http(&mut self, listener: TcpListener) -> SocketAddr {
        let address = listener
            .local_addr()
            .expect("bound listener has an address");
        let router = self.router();
        self.tasks.push(tokio::spawn(async move {
            let service = router.into_make_service_with_connect_info::<SocketAddr>();
            if let Err(error) = axum::serve(listener, service).await {
                warn!(%error, "http server stopped");
            }
        }));
        address
    }

    /// Closes every sync session, stops background tasks, flushes and closes
    /// the repository, and releases the database lock.
    pub async fn shutdown(mut self) -> Result<()> {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
        let result = self.app.close().await;
        let _ = std::fs::remove_file(&self.socket);
        drop(self.lock.take());
        result
    }
}
