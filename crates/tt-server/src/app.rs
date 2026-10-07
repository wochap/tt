//! Server state: database, repository, websocket transport, ACL cache, login
//! limiter, and the registry of open sync sessions.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use automerge_repo::{
    DocHandle, DocumentId, DocumentStatus, Error as RepoError, Repo, RepoConfig, SqliteStorage,
    transport::WsJsServer,
};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};
use tracing::{info, warn};

use crate::{
    acl::{AclCache, AclPolicy},
    auth::RateLimiter,
    db::Db,
};

/// The repository's `senderId`.
pub const SERVER_PEER_ID: &str = "tt-server";

#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// `server.db`: accounts and documents.
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
    /// How often open sessions are checked for tokens revoked by another
    /// process (`tt-server token revoke`).
    pub revocation_poll: Duration,
    /// Take the client address from `X-Forwarded-For`/`X-Real-IP` (only
    /// behind a trusted reverse proxy).
    pub behind_proxy: bool,
    /// Login attempts allowed per IP per `login_window`.
    pub login_limit: usize,
    pub login_window: Duration,
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
        }
    }
}

pub(crate) struct Session {
    pub token_hash: String,
    pub user_id: String,
    pub close: Arc<Notify>,
}

pub struct App {
    pub(crate) options: ServerOptions,
    pub(crate) db: Db,
    pub(crate) repo: Repo,
    pub(crate) transport: Arc<WsJsServer>,
    pub(crate) acl: Arc<AclCache>,
    pub(crate) limiter: RateLimiter,
    sessions: Mutex<HashMap<u64, Session>>,
    next_session: AtomicU64,
}

impl App {
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
        let sessions = self.sessions.lock().unwrap();
        let mut closed = 0;
        for session in sessions.values().filter(|s| s.token_hash == token_hash) {
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

    /// A stored document, or `None` when the server has never stored it.
    /// Never creates a placeholder or asks peers.
    pub(crate) async fn stored(&self, id: DocumentId) -> Result<Option<DocHandle>> {
        match self.repo.open_document(id).await {
            Ok(handle) if handle.status() == DocumentStatus::Ready => Ok(Some(handle)),
            Ok(_) | Err(RepoError::NotFound(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Document ids the user's index lists (workspace and entries), as the
    /// server currently holds the index.
    pub(crate) async fn index_listing(&self, index_doc: &str) -> Result<Vec<DocumentId>> {
        let index = DocumentId::parse_any(index_doc)?;
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
}

/// A running server: state plus background tasks. Bind it with
/// [`Server::spawn_http`] (tests, reverse proxy) or through `serve`.
pub struct Server {
    app: Arc<App>,
    tasks: Vec<JoinHandle<()>>,
}

impl Server {
    /// Opens the database and repository (documents load lazily) and starts
    /// the revocation poller.
    pub async fn open(options: ServerOptions) -> Result<Self> {
        let db = Db::open(&options.db)?;
        let storage = Arc::new(
            SqliteStorage::open(&options.db)
                .with_context(|| format!("opening {}", options.db.display()))?,
        );
        let acl = AclCache::new(db.clone());
        let transport = WsJsServer::new(SERVER_PEER_ID);
        let repo = Repo::open_with_policy(
            storage.clone(),
            storage,
            transport.clone(),
            RepoConfig {
                lazy_load: true,
                idle_eviction: Some(options.idle_eviction),
                ..RepoConfig::default()
            },
            Arc::new(AclPolicy(acl.clone())),
        )
        .await?;
        let app = Arc::new(App {
            limiter: RateLimiter::new(options.login_limit, options.login_window),
            options,
            db,
            repo,
            transport,
            acl,
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicU64::new(1),
        });
        let tasks = vec![tokio::spawn(app.clone().poll_revocations())];
        Ok(Self { app, tasks })
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

    /// Closes every sync session, stops background tasks, and flushes and
    /// closes the repository.
    pub async fn shutdown(self) -> Result<()> {
        for task in &self.tasks {
            task.abort();
        }
        self.app.close_all_sessions().await;
        self.app.repo.clone().shutdown().await?;
        Ok(())
    }
}
