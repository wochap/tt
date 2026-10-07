//! `tt daemon`: owns the Automerge repository and SQLite store, serves the
//! unix socket RPC, derives domain events, runs hooks, and syncs to a server
//! when one is configured.

pub mod bus;
pub mod config;
pub mod engine;
pub mod hooks;
pub mod net;
pub mod paths;
pub mod rpc;
pub mod server;

use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, Write},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use automerge_repo::{DocumentId, PeerId, Repo, RepoConfig, SqliteStorage, storage::ControlStore};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify, RwLock, mpsc, watch};
use tracing::{error, info, warn};
use tt_core::model::User;

use crate::{
    bus::Bus,
    config::Config,
    engine::Engine,
    hooks::Hooks,
    net::SyncTransport,
    paths::{Paths, ensure_private_dir},
    rpc::{RpcError, UNAVAILABLE},
};

/// Exit code when another daemon holds the lock.
pub const EXIT_LOCKED: i32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Readiness {
    Loading,
    Ready,
    Failed(String),
}

/// Shared daemon state behind the socket server.
pub struct Daemon {
    paths: Paths,
    repo: Repo,
    transport: Arc<SyncTransport>,
    engine: RwLock<Option<Arc<Mutex<Engine>>>>,
    readiness: watch::Sender<Readiness>,
    bus: Bus,
    hooks: Hooks,
    remote: mpsc::UnboundedSender<DocumentId>,
    started: Instant,
    shutdown: Notify,
}

impl Daemon {
    /// The engine, waiting up to 60 s while documents load.
    pub async fn engine_ready(&self) -> Result<Arc<Mutex<Engine>>, RpcError> {
        let mut readiness = self.readiness.subscribe();
        let waited = tokio::time::timeout(Duration::from_secs(60), async {
            readiness
                .wait_for(|state| *state != Readiness::Loading)
                .await
                .map(|state| state.clone())
        })
        .await;
        let state = match waited {
            Ok(Ok(state)) => state,
            _ => {
                return Err(RpcError::new(
                    UNAVAILABLE,
                    "daemon is still loading the workspace (waiting for sync?)",
                ));
            }
        };
        match state {
            Readiness::Ready => self
                .engine
                .read()
                .await
                .clone()
                .ok_or_else(|| RpcError::new(UNAVAILABLE, "engine not initialized")),
            Readiness::Failed(message) => Err(RpcError::new(UNAVAILABLE, message)),
            Readiness::Loading => unreachable!("waited for a non-loading state"),
        }
    }

    pub async fn handle(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "ping" => Ok(json!({"ok": true, "pid": std::process::id()})),
            "status" => Ok(self.status().await),
            "sync.reload" | "config.reload" => self.reload().await,
            "shutdown" => {
                self.shutdown.notify_one();
                Ok(json!({"ok": true}))
            }
            _ => {
                let engine = self.engine_ready().await?;
                let mut engine = engine.lock().await;
                engine.dispatch(method, params).await
            }
        }
    }

    async fn status(&self) -> Value {
        let readiness = match &*self.readiness.borrow() {
            Readiness::Loading => json!("loading"),
            Readiness::Ready => json!("ready"),
            Readiness::Failed(message) => json!({"failed": message}),
        };
        let engine = match self.engine.read().await.clone() {
            Some(engine) => engine.lock().await.status().await,
            None => Value::Null,
        };
        json!({
            "daemon": {
                "pid": std::process::id(),
                "version": env!("CARGO_PKG_VERSION"),
                "uptime_seconds": self.started.elapsed().as_secs(),
                "socket": self.paths.socket,
                "database": self.paths.database(),
                "log": self.paths.log_file(),
                "hooks": self.paths.hooks_dir(),
                "peer_id": self.repo.local_peer().to_string(),
                "state": readiness,
            },
            "sync": self.transport.status(),
            "workspace": engine,
        })
    }

    /// Re-reads config: reconnects sync and switches index if it changed.
    async fn reload(self: &Daemon) -> Result<Value, RpcError> {
        let config = Config::load(&self.paths.config_file()).map_err(RpcError::internal)?;
        match config.sync_endpoint() {
            Some((url, token)) => self.transport.connect(&url, &token).await,
            None => self.transport.disconnect().await,
        }
        let current = match self.engine.read().await.clone() {
            Some(engine) => Some(engine.lock().await.index_id()),
            None => None,
        };
        let wanted = config
            .user
            .index_doc
            .as_deref()
            .and_then(|id| DocumentId::parse_any(id).ok());
        let switched = wanted.is_some() && wanted != current;
        if switched {
            warn!(
                ?current,
                ?wanted,
                "index document changed; reloading workspace"
            );
            self.readiness.send_replace(Readiness::Loading);
            *self.engine.write().await = None;
        }
        Ok(json!({"sync": self.transport.status(), "reloading": switched}))
    }
}

async fn initialize(daemon: Arc<Daemon>) {
    let config = match Config::load(&daemon.paths.config_file()) {
        Ok(config) => config,
        Err(error) => {
            daemon
                .readiness
                .send_replace(Readiness::Failed(format!("{error:#}")));
            return;
        }
    };
    let user = User {
        id: config.user.id.clone().unwrap_or_else(|| "local".into()),
        name: config
            .user
            .name
            .clone()
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "me".into()),
    };
    let result = Engine::init(
        daemon.repo.clone(),
        config.user.index_doc.as_deref(),
        user,
        daemon.bus.clone(),
        daemon.hooks.clone(),
        daemon.remote.clone(),
    )
    .await;
    match result {
        Ok(init) => {
            if let Some(index) = init.created_index {
                let path = daemon.paths.config_file();
                let saved = Config::load(&path).and_then(|mut fresh| {
                    fresh.user.index_doc = Some(index.clone());
                    fresh.save(&path)
                });
                if let Err(error) = saved {
                    error!(%error, "could not record the new index document in config");
                }
            }
            info!(index = %init.engine.index_id().to_bs58check(), "workspace ready");
            *daemon.engine.write().await = Some(Arc::new(Mutex::new(init.engine)));
            daemon.readiness.send_replace(Readiness::Ready);
        }
        Err(error) => {
            error!(error = %format!("{error:#}"), "workspace failed to load");
            daemon
                .readiness
                .send_replace(Readiness::Failed(format!("{error:#}")));
        }
    }
}

/// Coalesces remote change notifications and applies them to the engine.
async fn remote_changes(daemon: Arc<Daemon>, mut queue: mpsc::UnboundedReceiver<DocumentId>) {
    while let Some(first) = queue.recv().await {
        let mut docs = BTreeSet::from([first]);
        tokio::time::sleep(Duration::from_millis(30)).await;
        while let Ok(more) = queue.try_recv() {
            docs.insert(more);
        }
        let Some(engine) = daemon.engine.read().await.clone() else {
            continue;
        };
        let mut engine = engine.lock().await;
        if let Err(error) = engine.on_remote(&docs).await {
            warn!(%error, "applying remote changes failed");
        }
    }
}

/// Watches readiness: when a reload drops the engine, initialize again.
async fn reinitialize_on_reload(daemon: Arc<Daemon>) {
    let mut readiness = daemon.readiness.subscribe();
    loop {
        if readiness.changed().await.is_err() {
            return;
        }
        let loading = *readiness.borrow_and_update() == Readiness::Loading;
        if loading && daemon.engine.read().await.is_none() {
            initialize(daemon.clone()).await;
        }
    }
}

fn init_logging(paths: &Paths, spawned: bool) -> Result<()> {
    use tracing_subscriber::{EnvFilter, fmt::writer::MakeWriterExt};
    ensure_private_dir(&paths.state_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_file())
        .with_context(|| format!("opening {}", paths.log_file().display()))?;
    let filter = EnvFilter::try_from_env("TT_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file = std::sync::Mutex::new(file);
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    let result = if spawned {
        builder.with_writer(file).try_init()
    } else {
        builder.with_writer(file.and(std::io::stderr)).try_init()
    };
    result.map_err(|error| anyhow::anyhow!("logging: {error}"))
}

/// Takes the exclusive lock or reports the holder.
fn lock(paths: &Paths) -> Result<std::result::Result<File, String>> {
    let path = paths.lock_file();
    let mut file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {
            file.set_len(0)?;
            file.rewind()?;
            writeln!(file, "{}", std::process::id())?;
            file.sync_all()?;
            Ok(Ok(file))
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            let mut holder = String::new();
            let _ = file.read_to_string(&mut holder);
            Ok(Err(format!(
                "another tt daemon holds {} (pid {})",
                path.display(),
                holder.trim()
            )))
        }
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

async fn peer_id(store: &SqliteStorage) -> Result<PeerId> {
    if let Some(bytes) = store.get("peer-id").await? {
        return Ok(PeerId::from(String::from_utf8_lossy(&bytes).into_owned()));
    }
    let id = format!("tt-{}", uuid::Uuid::new_v4().simple());
    store.put("peer-id", id.clone().into_bytes()).await?;
    ControlStore::flush(store).await?;
    Ok(PeerId::from(id))
}

/// Runs the daemon until SIGINT/SIGTERM or a `shutdown` request. Returns the
/// process exit code.
pub async fn run(spawned: bool) -> Result<i32> {
    let paths = Paths::from_env();
    ensure_private_dir(&paths.data_dir)?;
    ensure_private_dir(&paths.config_dir)?;
    init_logging(&paths, spawned)?;
    let lock = match lock(&paths)? {
        Ok(file) => file,
        Err(message) => {
            eprintln!("tt daemon: {message}");
            return Ok(EXIT_LOCKED);
        }
    };
    let listener = server::bind(&paths.socket)
        .with_context(|| format!("binding {}", paths.socket.display()))?;
    let store = SqliteStorage::open(paths.database())?;
    let peer = peer_id(&store).await?;
    let transport = SyncTransport::new(peer.clone());
    let store = Arc::new(store);
    let repo = Repo::open(
        store.clone(),
        store.clone(),
        transport.clone(),
        RepoConfig::default(),
    )
    .await?;
    let config = Config::load(&paths.config_file())?;
    match config.sync_endpoint() {
        Some((url, token)) => {
            info!(%url, "sync enabled");
            transport.connect(&url, &token).await;
        }
        None => info!("no server configured; running offline"),
    }
    let (remote, remote_rx) = mpsc::unbounded_channel();
    let (readiness, _) = watch::channel(Readiness::Loading);
    let daemon = Arc::new(Daemon {
        hooks: Hooks::start(paths.hooks_dir()),
        paths: paths.clone(),
        repo: repo.clone(),
        transport,
        engine: RwLock::new(None),
        readiness,
        bus: Bus::default(),
        remote,
        started: Instant::now(),
        shutdown: Notify::new(),
    });
    info!(pid = std::process::id(), socket = %paths.socket.display(), %peer, "daemon started");
    tokio::spawn(server::serve(listener, daemon.clone()));
    tokio::spawn(remote_changes(daemon.clone(), remote_rx));
    tokio::spawn(reinitialize_on_reload(daemon.clone()));
    tokio::spawn(initialize(daemon.clone()));

    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("interrupted"),
        _ = terminate.recv() => info!("terminated"),
        () = daemon.shutdown.notified() => info!("shutdown requested"),
    }
    if let Err(error) = repo.flush().await {
        error!(%error, "final flush failed");
    }
    let _ = std::fs::remove_file(&paths.socket);
    if let Err(error) = repo.shutdown().await {
        warn!(%error, "repository shutdown reported failures");
    }
    drop(lock);
    info!("daemon stopped");
    Ok(0)
}
