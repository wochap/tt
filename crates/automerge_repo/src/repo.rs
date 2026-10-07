//! Repository coordinator: document actors, persistence, and peer routing over
//! the automerge-repo JS protocol.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};

use automerge::{Automerge, sync::Message as SyncMessage, transaction::Transaction};
use bytes::Bytes;
use tokio::{
    sync::{RwLock, broadcast, mpsc, oneshot, watch},
    time::Instant,
};

use crate::{
    DocHandle, DocumentId, Error, PeerId, Result,
    document::{ActorConfig, ActorHandle, ActorOutput, DocumentStatus, InitJob, spawn_actor},
    error::{Failure, FailurePhase, LifecycleError, NetworkError, ProtocolError, StorageError},
    lifecycle::{CaptureGate, Lifecycle},
    network::{NetworkEvent, NetworkTransport},
    policy::{AccessPolicy, AllowAll},
    protocol::WireMessage,
    storage::{ControlStore, StorageAdapter},
    sync::{PeerSyncProgress, PeerSyncState, RelationshipSyncState},
};

#[derive(Clone, Debug)]
pub struct RepoConfig {
    /// Coordinator command and actor-output queue capacity.
    pub coordinator_capacity: usize,
    /// Capacity of each independent document actor mailbox.
    pub document_capacity: usize,
    /// Retained change events per document.
    pub event_capacity: usize,
    /// Retained asynchronous repository errors.
    pub error_capacity: usize,
    /// Delay used to coalesce automatic snapshots after a head change.
    pub persistence_debounce: Duration,
    /// Initial retry delay after an automatic snapshot failure.
    pub persistence_retry_min: Duration,
    /// Maximum exponential automatic snapshot retry delay.
    pub persistence_retry_max: Duration,
    /// Capacity of each connected peer's private writer queue.
    pub peer_writer_capacity: usize,
    /// Open lists stored documents without loading them; each is loaded on
    /// first use (`find`, `open_document`, or a peer's sync/request).
    pub lazy_load: bool,
    /// Closes a `Ready` document after this long without activity, once no
    /// connected peer is attached to it and no [`DocHandle`] is held outside
    /// the repository. Its snapshot is flushed first; it reloads on demand.
    pub idle_eviction: Option<Duration>,
}
impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            coordinator_capacity: 128,
            document_capacity: 64,
            event_capacity: 128,
            error_capacity: 128,
            persistence_debounce: Duration::from_millis(50),
            persistence_retry_min: Duration::from_millis(100),
            persistence_retry_max: Duration::from_secs(5),
            peer_writer_capacity: 256,
            lazy_load: false,
            idle_eviction: None,
        }
    }
}

impl RepoConfig {
    fn validate(&self) -> Result<()> {
        if self.coordinator_capacity == 0
            || self.document_capacity == 0
            || self.event_capacity == 0
            || self.error_capacity == 0
            || self.peer_writer_capacity == 0
        {
            return Err(Error::Config(
                "all queue capacities must be greater than zero".into(),
            ));
        }
        if self.persistence_retry_min.is_zero() {
            return Err(Error::Config(
                "persistence_retry_min must be greater than zero".into(),
            ));
        }
        if self.idle_eviction.is_some_and(|idle| idle.is_zero()) {
            return Err(Error::Config(
                "idle_eviction must be greater than zero".into(),
            ));
        }
        if self.persistence_retry_min > self.persistence_retry_max {
            return Err(Error::Config(
                "persistence_retry_min must not exceed persistence_retry_max".into(),
            ));
        }
        Ok(())
    }
}

fn actor_config(config: &RepoConfig) -> ActorConfig {
    ActorConfig {
        mailbox: config.document_capacity,
        events: config.event_capacity,
        debounce: config.persistence_debounce,
        retry_min: config.persistence_retry_min,
        retry_max: config.persistence_retry_max,
    }
}

enum Command {
    DocumentIds(oneshot::Sender<Result<Vec<DocumentId>>>),
    Get(DocumentId, oneshot::Sender<Result<Option<DocHandle>>>),
    Open(DocumentId, oneshot::Sender<Result<DocHandle>>),
    Find(DocumentId, oneshot::Sender<Result<DocHandle>>),
    Create(Option<InitJob>, oneshot::Sender<Result<DocHandle>>),
    Flush(oneshot::Sender<Result<()>>),
    Remove(DocumentId, oneshot::Sender<Result<()>>),
    Announce(DocumentId, oneshot::Sender<Result<()>>),
    Shutdown(oneshot::Sender<Result<()>>),
}

/// Cloneable handle to the single repository coordinator.
#[derive(Clone)]
pub struct Repo {
    tx: mpsc::Sender<Command>,
    local_peer: PeerId,
    peers: watch::Receiver<Vec<PeerId>>,
    peer_sync: watch::Receiver<HashMap<PeerId, PeerSyncProgress>>,
    errors: broadcast::Sender<Error>,
    lifecycle: Lifecycle,
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("local_peer", &self.local_peer)
            .finish_non_exhaustive()
    }
}

impl Repo {
    /// Opens stored documents strictly with the allow-all access policy.
    pub async fn open(
        storage: Arc<dyn StorageAdapter>,
        control: Arc<dyn ControlStore>,
        transport: Arc<dyn NetworkTransport>,
        config: RepoConfig,
    ) -> Result<Self> {
        Self::open_with_policy(storage, control, transport, config, Arc::new(AllowAll)).await
    }

    /// Opens stored documents strictly, then starts the coordinator and the
    /// network event reader. Every stored snapshot must load; the transport
    /// event receiver is consumed exactly once, after validation.
    pub async fn open_with_policy(
        storage: Arc<dyn StorageAdapter>,
        control: Arc<dyn ControlStore>,
        transport: Arc<dyn NetworkTransport>,
        config: RepoConfig,
        policy: Arc<dyn AccessPolicy>,
    ) -> Result<Self> {
        config.validate()?;
        let ids = storage.list().await?;
        let mut documents = HashMap::new();
        for id in ids.iter().filter(|_| !config.lazy_load) {
            let bytes = storage.load(*id).await?.ok_or_else(|| {
                StorageError::new("load", Some(*id), "listed snapshot disappeared")
            })?;
            let doc = Automerge::load(&bytes).map_err(|error| Error::Automerge {
                document: *id,
                message: error.to_string(),
            })?;
            documents.insert(*id, doc);
        }
        let lifecycle = Lifecycle::new();
        let capture_gate: CaptureGate = Arc::new(RwLock::new(()));
        let (peers_tx, peers) = watch::channel(Vec::new());
        let (peer_sync_tx, peer_sync) = watch::channel(HashMap::new());
        let (errors, _) = broadcast::channel(config.error_capacity.max(1));
        let (actor_tx, actor_rx) = mpsc::channel(config.coordinator_capacity.max(1));
        let mut actors = HashMap::new();
        let now = Instant::now();
        let last_active = documents.keys().map(|id| (*id, now)).collect();
        for (id, doc) in documents {
            let status = if doc.get_heads().is_empty() {
                DocumentStatus::Loading
            } else {
                DocumentStatus::Ready
            };
            actors.insert(
                id,
                spawn_actor(
                    id,
                    doc,
                    status,
                    actor_config(&config),
                    actor_tx.clone(),
                    errors.clone(),
                    storage.clone(),
                    lifecycle.clone(),
                    capture_gate.clone(),
                ),
            );
        }
        let events = transport.take_events()?;
        let local_peer = transport.local_peer();
        let (tx, commands) = mpsc::channel(config.coordinator_capacity.max(1));
        let coordinator = Coordinator {
            storage,
            control,
            transport,
            policy,
            config,
            local_peer: local_peer.clone(),
            actors,
            stored: ids.into_iter().collect(),
            last_active,
            peers: HashMap::new(),
            peers_tx,
            peer_sync_tx,
            relationships: HashMap::new(),
            unavailable: HashMap::new(),
            errors: errors.clone(),
            actor_tx,
            lifecycle: lifecycle.clone(),
            capture_gate,
        };
        tokio::spawn(coordinator.run(commands, events, actor_rx));
        Ok(Self {
            tx,
            local_peer,
            peers,
            peer_sync,
            errors,
            lifecycle,
        })
    }

    /// The id this repository uses as `senderId`.
    #[must_use]
    pub fn local_peer(&self) -> &PeerId {
        &self.local_peer
    }
    /// Watches the set of peers with a completed handshake.
    #[must_use]
    pub fn subscribe_peers(&self) -> watch::Receiver<Vec<PeerId>> {
        self.peers.clone()
    }
    #[must_use]
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers.borrow().clone()
    }
    #[must_use]
    /// Watches retained whole-repository synchronization progress per peer.
    pub fn subscribe_peer_sync(&self) -> watch::Receiver<HashMap<PeerId, PeerSyncProgress>> {
        self.peer_sync.clone()
    }
    #[must_use]
    /// Returns the latest retained synchronization snapshot.
    pub fn peer_sync_progress(&self) -> HashMap<PeerId, PeerSyncProgress> {
        self.peer_sync.borrow().clone()
    }
    #[must_use]
    /// Subscribes to typed asynchronous persistence, network, and protocol errors.
    pub fn subscribe_errors(&self) -> broadcast::Receiver<Error> {
        self.errors.subscribe()
    }
    /// Lists every known document ID in deterministic order.
    pub async fn document_ids(&self) -> Result<Vec<DocumentId>> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, Command::DocumentIds).await
    }
    /// Returns a cached document handle, without fabricating a placeholder.
    pub async fn get(&self, id: DocumentId) -> Result<Option<DocHandle>> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| Command::Get(id, reply)).await
    }
    /// Opens a stored document or returns `Error::NotFound`.
    pub async fn open_document(&self, id: DocumentId) -> Result<DocHandle> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| Command::Open(id, reply)).await
    }
    /// Returns the local document, or a `Loading` placeholder that requests it
    /// from every connected peer the policy allows. Await
    /// [`DocHandle::ready`] (with a timeout) for its content.
    pub async fn find(&self, id: DocumentId) -> Result<DocHandle> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| Command::Find(id, reply)).await
    }
    /// Creates an empty document with explicit synchronizable history.
    pub async fn create(&self) -> Result<DocHandle> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| Command::Create(None, reply)).await
    }
    /// Creates a document and atomically applies its initializer before sharing.
    pub async fn create_with<F>(&self, initialize: F) -> Result<DocHandle>
    where
        F: FnOnce(&mut Transaction<'_>) -> Result<()> + Send + 'static,
    {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| {
            Command::Create(Some(Box::new(initialize)), reply)
        })
        .await
    }
    /// Captures all actor revisions at one linearization point and aggregates
    /// document and storage barrier failures.
    pub async fn flush(&self) -> Result<()> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, Command::Flush).await
    }
    /// Atomically stops admission, drains accepted work, best-effort flushes,
    /// closes every subsystem, and returns all phase-tagged failures.
    pub async fn shutdown(self) -> Result<()> {
        self.lifecycle.begin_shutdown()?;
        request(&self.tx, Command::Shutdown).await
    }
    /// Durably removes one local snapshot without protocol deletion.
    pub async fn remove_local(&self, id: DocumentId) -> Result<()> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| Command::Remove(id, reply)).await
    }
    /// Pushes a loaded document to every connected peer the policy now lets
    /// it be announced to. Use after a policy starts allowing a document that
    /// already exists; a document that is not loaded is left alone.
    pub async fn announce(&self, id: DocumentId) -> Result<()> {
        self.lifecycle.ensure_open()?;
        request(&self.tx, |reply| Command::Announce(id, reply)).await
    }
}

trait MakeCommand<T> {
    fn make(self, reply: oneshot::Sender<Result<T>>) -> Command;
}
impl<T, F: FnOnce(oneshot::Sender<Result<T>>) -> Command> MakeCommand<T> for F {
    fn make(self, reply: oneshot::Sender<Result<T>>) -> Command {
        self(reply)
    }
}
async fn tick(interval: Option<&mut tokio::time::Interval>) {
    match interval {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending::<()>().await,
    }
}

async fn request<T>(tx: &mpsc::Sender<Command>, command: impl MakeCommand<T>) -> Result<T> {
    let (reply, receive) = oneshot::channel();
    tx.send(command.make(reply))
        .await
        .map_err(|_| LifecycleError::RepositoryClosed)?;
    receive
        .await
        .map_err(|_| LifecycleError::RepositoryClosed)?
}

struct PeerState {
    writer: mpsc::Sender<Bytes>,
    writer_task: tokio::task::JoinHandle<()>,
}

struct Coordinator {
    storage: Arc<dyn StorageAdapter>,
    control: Arc<dyn ControlStore>,
    transport: Arc<dyn NetworkTransport>,
    policy: Arc<dyn AccessPolicy>,
    config: RepoConfig,
    local_peer: PeerId,
    actors: HashMap<DocumentId, ActorHandle>,
    /// Every document known to storage or created here, loaded or not.
    stored: BTreeSet<DocumentId>,
    /// Last activity per loaded document, for idle eviction.
    last_active: HashMap<DocumentId, Instant>,
    peers: HashMap<PeerId, PeerState>,
    peers_tx: watch::Sender<Vec<PeerId>>,
    peer_sync_tx: watch::Sender<HashMap<PeerId, PeerSyncProgress>>,
    relationships: HashMap<(PeerId, DocumentId), RelationshipSyncState>,
    /// Peers that answered `doc-unavailable` for a document we lack.
    unavailable: HashMap<DocumentId, Vec<PeerId>>,
    errors: broadcast::Sender<Error>,
    actor_tx: mpsc::Sender<ActorOutput>,
    lifecycle: Lifecycle,
    capture_gate: CaptureGate,
}

impl Coordinator {
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut events: mpsc::Receiver<NetworkEvent>,
        mut actor_output: mpsc::Receiver<ActorOutput>,
    ) {
        let mut events_open = true;
        let mut sweep = self.config.idle_eviction.map(|idle| {
            let mut interval = tokio::time::interval((idle / 4).max(Duration::from_millis(10)));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval
        });
        loop {
            tokio::select! {
                Some(command) = commands.recv() => {
                    if let Command::Shutdown(reply) = command {
                        commands.close();
                        while let Some(accepted) = commands.recv().await {
                            let _ = self.handle_command(accepted).await;
                        }
                        let result = self.shutdown_all().await;
                        let _ = reply.send(result);
                        break;
                    }
                    if self.handle_command(command).await { break; }
                },
                event = events.recv(), if events_open => match event {
                    Some(event) => self.handle_network(event).await,
                    None => events_open = false,
                },
                Some(output) = actor_output.recv() => {
                    self.handle_actor_output(output).await;
                }
                _ = tick(sweep.as_mut()) => self.evict_idle().await,
                else => break,
            }
        }
    }

    async fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::DocumentIds(reply) => {
                let mut ids: BTreeSet<_> = self.stored.clone();
                ids.extend(self.actors.keys().copied());
                let _ = reply.send(Ok(ids.into_iter().collect()));
            }
            Command::Get(id, reply) => {
                let _ = reply.send(Ok(self.actors.get(&id).map(|actor| actor.handle.clone())));
            }
            Command::Open(id, reply) => {
                let _ = reply.send(self.open_document(id).await);
            }
            Command::Find(id, reply) => {
                let _ = reply.send(self.find(id).await);
            }
            Command::Create(job, reply) => {
                let _ = reply.send(self.create(job).await);
            }
            Command::Flush(reply) => {
                let _ = reply.send(self.flush_all().await);
            }
            Command::Remove(id, reply) => {
                let _ = reply.send(self.remove_local(id).await);
            }
            Command::Announce(id, reply) => {
                self.touch(id);
                self.announce(id).await;
                let _ = reply.send(Ok(()));
            }
            Command::Shutdown(reply) => {
                let result = self.shutdown_all().await;
                let _ = reply.send(result);
                return true;
            }
        }
        false
    }

    async fn open_document(&mut self, id: DocumentId) -> Result<DocHandle> {
        self.touch(id);
        if let Some(actor) = self.actors.get(&id) {
            return Ok(actor.handle.clone());
        }
        let bytes = self.storage.load(id).await?.ok_or(Error::NotFound(id))?;
        let doc = Automerge::load(&bytes).map_err(|error| Error::Automerge {
            document: id,
            message: error.to_string(),
        })?;
        let status = if doc.get_heads().is_empty() {
            DocumentStatus::Loading
        } else {
            DocumentStatus::Ready
        };
        let actor = self.spawn(id, doc, status);
        let handle = actor.handle.clone();
        self.insert_actor(id, actor);
        self.announce(id).await;
        Ok(handle)
    }

    fn insert_actor(&mut self, id: DocumentId, actor: ActorHandle) {
        self.actors.insert(id, actor);
        self.stored.insert(id);
        self.touch(id);
    }

    fn touch(&mut self, id: DocumentId) {
        if self.config.idle_eviction.is_some() {
            self.last_active.insert(id, Instant::now());
        }
    }

    /// Closes `Ready` documents idle for the configured period that no
    /// connected peer is attached to and no outside handle references.
    async fn evict_idle(&mut self) {
        let Some(idle) = self.config.idle_eviction else {
            return;
        };
        let now = Instant::now();
        let attached: BTreeSet<DocumentId> = self
            .relationships
            .keys()
            .map(|(_, document)| *document)
            .collect();
        let idle_ids: Vec<DocumentId> = self
            .actors
            .iter()
            .filter(|(id, actor)| {
                actor.handle.status() == DocumentStatus::Ready
                    && !attached.contains(id)
                    && !actor.held_outside()
                    && self
                        .last_active
                        .get(id)
                        .is_none_or(|last| now.duration_since(*last) >= idle)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in idle_ids {
            let Some(actor) = self.actors.remove(&id) else {
                continue;
            };
            self.last_active.remove(&id);
            self.unavailable.remove(&id);
            // Close persists any pending revision before the actor stops.
            if let Err(error) = actor.close().await {
                let _ = self.errors.send(error);
            }
        }
    }

    async fn find(&mut self, id: DocumentId) -> Result<DocHandle> {
        match self.open_document(id).await {
            Err(Error::NotFound(_)) => {}
            other => return other,
        }
        let actor = self.spawn(id, Automerge::new(), DocumentStatus::Loading);
        let handle = actor.handle.clone();
        self.insert_actor(id, actor);
        self.request_from_peers(id).await;
        Ok(handle)
    }

    /// Attaches every connected peer the policy allows to a document we lack;
    /// the actor then emits `request` messages. With no such peer the
    /// document is immediately unavailable.
    async fn request_from_peers(&mut self, id: DocumentId) {
        let peers: Vec<_> = self
            .peers
            .keys()
            .filter(|peer| self.policy.may_sync(peer, id))
            .cloned()
            .collect();
        let Some(actor) = self.actors.get(&id) else {
            return;
        };
        if peers.is_empty() {
            actor.set_availability(false).await;
            return;
        }
        actor.set_availability(true).await;
        for peer in peers {
            let _ = actor.attach(peer).await;
        }
    }

    async fn create(&mut self, initialize: Option<InitJob>) -> Result<DocHandle> {
        let id = DocumentId::new();
        let actor = self.spawn(id, Automerge::new(), DocumentStatus::Loading);
        if let Err(primary) = actor.initialize_hidden(initialize).await {
            let _ = actor.begin_remove().await;
            return Err(self.creation_failure(id, primary).await);
        }
        let handle = actor.handle.clone();
        self.insert_actor(id, actor);
        Ok(handle)
    }

    async fn creation_failure(&self, id: DocumentId, primary: Error) -> Error {
        let mut cleanup = Vec::new();
        if let Err(error) = self.storage.remove(id).await {
            cleanup.push(Failure {
                phase: FailurePhase::CleanupRemove,
                document: Some(id),
                revision: None,
                message: error.to_string(),
            });
        }
        if let Err(error) = self.storage.flush().await {
            cleanup.push(Failure {
                phase: FailurePhase::DocumentFlush,
                document: Some(id),
                revision: None,
                message: error.to_string(),
            });
        }
        Error::Creation {
            document: id,
            primary: Box::new(primary),
            cleanup,
        }
    }

    async fn remove_local(&mut self, id: DocumentId) -> Result<()> {
        if !self.actors.contains_key(&id) && self.stored.contains(&id) {
            // Lazily stored or evicted: load it so removal follows one path.
            self.open_document(id).await?;
        }
        let actor = self.actors.remove(&id).ok_or(Error::NotFound(id))?;
        self.stored.remove(&id);
        self.last_active.remove(&id);
        // Closing waits for the actor's one ordered persistence worker, ensuring
        // no late store can recreate a successfully removed snapshot.
        let _ = actor.begin_remove().await;
        self.unavailable.remove(&id);
        self.storage.remove(id).await?;
        self.storage.flush().await?;
        Ok(())
    }

    fn spawn(&self, id: DocumentId, doc: Automerge, status: DocumentStatus) -> ActorHandle {
        spawn_actor(
            id,
            doc,
            status,
            actor_config(&self.config),
            self.actor_tx.clone(),
            self.errors.clone(),
            self.storage.clone(),
            self.lifecycle.clone(),
            self.capture_gate.clone(),
        )
    }

    async fn flush_all(&mut self) -> Result<()> {
        // Acquiring the exclusive side is the repository-wide flush
        // linearization point. Actors publish each new revision while holding
        // the shared side of this same gate.
        let targets = {
            let _capture = self.capture_gate.write().await;
            self.actors
                .iter()
                .map(|(id, actor)| (*id, actor.clone(), actor.revision()))
                .collect::<Vec<_>>()
        };
        let mut joins = tokio::task::JoinSet::new();
        for (id, actor, target) in targets {
            joins.spawn(async move { (id, target, actor.flush(target).await) });
        }
        let mut failures = Vec::new();
        while let Some(joined) = joins.join_next().await {
            match joined {
                Ok((_, _, Ok(()))) => {}
                Ok((id, revision, Err(error))) => failures.push(Failure {
                    phase: FailurePhase::DocumentStore,
                    document: Some(id),
                    revision: Some(revision),
                    message: error.to_string(),
                }),
                Err(error) => failures.push(Failure {
                    phase: FailurePhase::DocumentStore,
                    document: None,
                    revision: None,
                    message: error.to_string(),
                }),
            }
        }
        if let Err(error) = self.storage.flush().await {
            failures.push(Failure {
                phase: FailurePhase::DocumentFlush,
                document: error.document,
                revision: error.revision,
                message: error.to_string(),
            });
        }
        failures.sort_by_key(|failure| (failure.phase, failure.document, failure.revision));
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Flush(failures))
        }
    }

    async fn shutdown_all(&mut self) -> Result<()> {
        let mut failures = match self.flush_all().await {
            Ok(()) => Vec::new(),
            Err(Error::Flush(items)) => items,
            Err(error) => vec![Failure {
                phase: FailurePhase::DocumentFlush,
                document: None,
                revision: None,
                message: error.to_string(),
            }],
        };
        for (id, actor) in &self.actors {
            if let Err(error) = actor.close().await {
                failures.push(Failure {
                    phase: FailurePhase::DocumentClose,
                    document: Some(*id),
                    revision: Some(actor.revision()),
                    message: error.to_string(),
                });
            }
        }
        for (_, state) in self.peers.drain() {
            state.writer_task.abort();
        }
        self.peers_tx.send_replace(Vec::new());
        if let Err(error) = self.transport.close().await {
            failures.push(Failure {
                phase: FailurePhase::TransportClose,
                document: None,
                revision: None,
                message: error.to_string(),
            });
        }
        if let Err(error) = self.storage.flush().await {
            failures.push(Failure {
                phase: FailurePhase::DocumentFlush,
                document: error.document,
                revision: error.revision,
                message: error.to_string(),
            });
        }
        if let Err(error) = self.storage.close().await {
            failures.push(Failure {
                phase: FailurePhase::DocumentClose,
                document: error.document,
                revision: error.revision,
                message: error.to_string(),
            });
        }
        if let Err(error) = self.control.flush().await {
            failures.push(Failure {
                phase: FailurePhase::ControlFlush,
                document: error.document,
                revision: error.revision,
                message: error.to_string(),
            });
        }
        if let Err(error) = self.control.close().await {
            failures.push(Failure {
                phase: FailurePhase::ControlClose,
                document: error.document,
                revision: error.revision,
                message: error.to_string(),
            });
        }
        self.lifecycle.close();
        failures.sort_by_key(|failure| (failure.phase, failure.document, failure.revision));
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Shutdown(failures))
        }
    }

    async fn handle_network(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::PeerConnected(peer) => {
                self.drop_peer(&peer).await;
                let (writer, mut frames) = mpsc::channel(self.config.peer_writer_capacity);
                let transport = self.transport.clone();
                let writer_peer = peer.clone();
                let output = self.actor_tx.clone();
                let writer_task = tokio::spawn(async move {
                    while let Some(frame) = frames.recv().await {
                        if let Err(error) = transport.send(&writer_peer, frame).await {
                            let _ = output
                                .send(ActorOutput::PeerWriterFailed(writer_peer.clone(), error))
                                .await;
                            return;
                        }
                    }
                });
                self.peers.insert(
                    peer.clone(),
                    PeerState {
                        writer,
                        writer_task,
                    },
                );
                self.publish_peers();
                self.peer_sync_tx.send_modify(|progress| {
                    progress.insert(
                        peer.clone(),
                        PeerSyncProgress {
                            peer: peer.clone(),
                            state: PeerSyncState::Connected,
                            documents: 0,
                            syncing_documents: Vec::new(),
                        },
                    );
                });
                let mut ids: Vec<_> = self.actors.keys().copied().collect();
                ids.sort();
                for id in ids {
                    let Some(actor) = self.actors.get(&id) else {
                        continue;
                    };
                    if actor.handle.status() == DocumentStatus::Ready {
                        if self.policy.may_announce(&peer, id) {
                            let _ = actor.attach(peer.clone()).await;
                        }
                    } else if self.policy.may_sync(&peer, id) {
                        // A document we are still looking for: ask the new peer.
                        actor.set_availability(true).await;
                        let _ = actor.attach(peer.clone()).await;
                    }
                }
            }
            NetworkEvent::PeerDisconnected(peer) => {
                self.drop_peer(&peer).await;
            }
            NetworkEvent::Message { peer, bytes } => match WireMessage::decode(&bytes) {
                Ok(message) => self.handle_message(peer, message).await,
                Err(error) => self.protocol_failure(peer, error).await,
            },
        }
    }

    async fn handle_message(&mut self, peer: PeerId, message: WireMessage) {
        if !self.peers.contains_key(&peer) {
            return;
        }
        match message {
            WireMessage::Sync {
                document_id, data, ..
            } => self.receive_sync(peer, document_id, data, false).await,
            WireMessage::Request {
                document_id, data, ..
            } => self.receive_sync(peer, document_id, data, true).await,
            WireMessage::DocUnavailable { document_id, .. } => {
                self.peer_lacks(peer, document_id).await;
            }
            WireMessage::Error { message, .. } => {
                let _ = self.errors.send(ProtocolError::Remote(message).into());
            }
            WireMessage::Join { .. } | WireMessage::Peer { .. } => {
                self.protocol_failure(
                    peer,
                    ProtocolError::UnexpectedHandshake(message.type_name().into()),
                )
                .await;
            }
            // Ephemeral messages and remote-heads bookkeeping are not used here.
            WireMessage::Ephemeral { .. } | WireMessage::Other { .. } => {}
        }
    }

    async fn receive_sync(&mut self, peer: PeerId, id: DocumentId, data: Vec<u8>, request: bool) {
        if !self.policy.may_sync(&peer, id) {
            self.send_unavailable(&peer, id).await;
            return;
        }
        let message = match SyncMessage::decode(&data) {
            Ok(message) => message,
            Err(_) => {
                self.protocol_failure(peer, ProtocolError::InvalidSyncPayload)
                    .await;
                return;
            }
        };
        if !self.actors.contains_key(&id) && (self.stored.contains(&id) || self.config.lazy_load) {
            // Not loaded yet (lazy open, evicted, or stored by another
            // process sharing the database): load before serving.
            match self.open_document(id).await {
                Ok(_) | Err(Error::NotFound(_)) => {}
                Err(error) => {
                    let _ = self.errors.send(error);
                    self.send_unavailable(&peer, id).await;
                    return;
                }
            }
        }
        self.touch(id);
        // A document still on its way to its first durable snapshot already
        // has history (a published revision) and can serve the requester.
        let have = self.actors.get(&id).is_some_and(|actor| {
            actor.handle.status() == DocumentStatus::Ready || actor.revision() > 0
        });
        if request && !have {
            // Neither side has it; this repository does not forward requests.
            self.send_unavailable(&peer, id).await;
            return;
        }
        if !self.actors.contains_key(&id) {
            let actor = self.spawn(id, Automerge::new(), DocumentStatus::Loading);
            self.insert_actor(id, actor);
        }
        if let Some(actor) = self.actors.get(&id)
            && let Err(error) = actor.receive(peer, message).await
        {
            let _ = self.errors.send(error);
        }
    }

    async fn peer_lacks(&mut self, peer: PeerId, id: DocumentId) {
        let Some(actor) = self.actors.get(&id) else {
            return;
        };
        if actor.handle.status() == DocumentStatus::Ready {
            return;
        }
        actor.detach(peer.clone()).await;
        let lacking = self.unavailable.entry(id).or_default();
        if !lacking.contains(&peer) {
            lacking.push(peer);
        }
        let everyone_lacks = self
            .peers
            .keys()
            .filter(|candidate| self.policy.may_sync(candidate, id))
            .all(|candidate| lacking.contains(candidate));
        if everyone_lacks {
            actor.set_availability(false).await;
        }
    }

    async fn send_unavailable(&mut self, peer: &PeerId, id: DocumentId) {
        let message = WireMessage::DocUnavailable {
            sender_id: self.local_peer.to_string(),
            target_id: peer.to_string(),
            document_id: id,
        };
        self.send(peer, &message).await;
    }

    fn publish_peers(&self) {
        let mut peers: Vec<_> = self.peers.keys().cloned().collect();
        peers.sort();
        self.peers_tx.send_replace(peers);
    }

    async fn drop_peer(&mut self, peer: &PeerId) {
        for actor in self.actors.values() {
            actor.detach(peer.clone()).await;
        }
        if let Some(state) = self.peers.remove(peer) {
            state.writer_task.abort();
        }
        for lacking in self.unavailable.values_mut() {
            lacking.retain(|candidate| candidate != peer);
        }
        self.relationships
            .retain(|(candidate, _), _| candidate != peer);
        self.peer_sync_tx.send_modify(|progress| {
            progress.remove(peer);
        });
        self.publish_peers();
    }

    async fn handle_actor_output(&mut self, output: ActorOutput) {
        match output {
            ActorOutput::Send {
                peer,
                document,
                message,
                request,
            } => {
                if !self.policy.may_sync(&peer, document) {
                    return;
                }
                self.touch(document);
                let sender_id = self.local_peer.to_string();
                let target_id = peer.to_string();
                let data = message.encode();
                let wire = if request {
                    WireMessage::Request {
                        sender_id,
                        target_id,
                        document_id: document,
                        data,
                    }
                } else {
                    WireMessage::Sync {
                        sender_id,
                        target_id,
                        document_id: document,
                        data,
                    }
                };
                self.send(&peer, &wire).await;
            }
            ActorOutput::DurableHistory(id) => {
                self.unavailable.remove(&id);
                self.announce(id).await;
            }
            ActorOutput::PeerWriterFailed(peer, error) => {
                let _ = self.errors.send(error.into());
                self.drop_peer(&peer).await;
            }
            ActorOutput::SyncProgress {
                peer,
                document,
                state,
            } => {
                match state {
                    Some(state) => {
                        self.relationships.insert((peer.clone(), document), state);
                    }
                    None => {
                        self.relationships.remove(&(peer.clone(), document));
                    }
                }
                self.refresh_peer_progress(&peer);
            }
        }
    }

    /// Attaches every peer the policy lets us push `id` to.
    async fn announce(&self, id: DocumentId) {
        if let Some(actor) = self.actors.get(&id) {
            for peer in self.peers.keys() {
                if self.policy.may_announce(peer, id) {
                    let _ = actor.attach(peer.clone()).await;
                }
            }
        }
    }

    fn refresh_peer_progress(&self, peer: &PeerId) {
        if !self.peers.contains_key(peer) {
            return;
        }
        let mut relationships: Vec<_> = self
            .relationships
            .iter()
            .filter(|((candidate, _), _)| candidate == peer)
            .map(|((_, document), state)| (*document, *state))
            .collect();
        relationships.sort_by_key(|(document, _)| *document);
        let syncing_documents = relationships
            .iter()
            .filter(|(_, state)| *state == RelationshipSyncState::Syncing)
            .map(|(document, _)| *document)
            .collect::<Vec<_>>();
        let state = if relationships.is_empty() {
            PeerSyncState::Connected
        } else if syncing_documents.is_empty() {
            PeerSyncState::Synced
        } else {
            PeerSyncState::Syncing
        };
        let value = PeerSyncProgress {
            peer: peer.clone(),
            state,
            documents: relationships.len(),
            syncing_documents,
        };
        self.peer_sync_tx.send_modify(|progress| {
            progress.insert(peer.clone(), value);
        });
    }

    async fn send(&mut self, peer: &PeerId, message: &WireMessage) {
        match message.encode() {
            Ok(frame) => {
                let queued = self
                    .peers
                    .get(peer)
                    .map(|state| state.writer.try_send(Bytes::from(frame)));
                if !matches!(queued, Some(Ok(()))) {
                    let error = NetworkError::Transport {
                        peer: peer.clone(),
                        message: "peer writer queue is full or closed".into(),
                    };
                    let _ = self.errors.send(error.into());
                    // The peer's sync states are now out of step; a fresh
                    // session is the only safe way back.
                    self.drop_peer(peer).await;
                    let _ = self.transport.close_peer(peer).await;
                }
            }
            Err(error) => {
                let _ = self.errors.send(error.into());
            }
        }
    }

    async fn protocol_failure(&mut self, peer: PeerId, error: ProtocolError) {
        let _ = self.errors.send(error.into());
        self.drop_peer(&peer).await;
        let _ = self.transport.close_peer(&peer).await;
    }
}
