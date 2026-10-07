//! Shared harness: an in-process server on a random port, HTTP helpers, and
//! Rust automerge-repo clients that authenticate with tickets.
#![allow(dead_code)]

use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use automerge_repo::{
    DocHandle, DocumentId, Repo, RepoConfig,
    testing::MemoryStore,
    transport::{
        AuthError, ConnectAuth, ConnectTarget, ConnectionState, WsJsClient, WsJsClientConfig,
    },
};
use serde_json::{Value, json};
use tokio::time::timeout;
use tt_server::{Server, ServerOptions, admin};

pub const PASSWORD: &str = "correct horse battery";

pub struct TestServer {
    pub dir: tempfile::TempDir,
    pub server: Option<Server>,
    pub addr: SocketAddr,
}

impl TestServer {
    pub async fn start(configure: impl FnOnce(&mut ServerOptions)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self::start_in(dir, configure).await
    }

    pub async fn start_in(
        dir: tempfile::TempDir,
        configure: impl FnOnce(&mut ServerOptions),
    ) -> Self {
        let mut options = ServerOptions::new(dir.path().join("server.db"));
        options.pending_timeout = Duration::from_millis(500);
        options.revocation_poll = Duration::from_millis(100);
        configure(&mut options);
        let mut server = Server::open(options).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.spawn_http(listener);
        Self {
            dir,
            server: Some(server),
            addr,
        }
    }

    /// Stops the server and returns its directory for a restart.
    pub async fn stop(mut self) -> tempfile::TempDir {
        self.server.take().unwrap().shutdown().await.unwrap();
        self.dir
    }

    pub fn db(&self) -> PathBuf {
        self.dir.path().join("server.db")
    }

    pub fn server(&self) -> &Server {
        self.server.as_ref().unwrap()
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn ws(&self) -> String {
        format!("ws://{}/sync", self.addr)
    }

    pub async fn add_user(&self, name: &str) -> tt_server::db::UserRecord {
        admin::add_user(&self.db(), name, PASSWORD).await.unwrap()
    }

    pub async fn login(&self, name: &str) -> Login {
        let (status, body) = post(
            &self.base(),
            "/api/login",
            None,
            json!({"username": name, "password": PASSWORD}),
        )
        .await;
        assert_eq!(status, 200, "login {name}: {body}");
        Login {
            token: body["token"].as_str().unwrap().to_owned(),
            index_doc: body["index_doc"].as_str().unwrap().to_owned(),
            user_id: body["user"]["id"].as_str().unwrap().to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Login {
    pub token: String,
    pub index_doc: String,
    pub user_id: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

fn read(mut response: ureq::http::Response<ureq::Body>) -> (u16, Value) {
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

pub async fn post(base: &str, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
    let url = format!("{base}{path}");
    let token = token.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let mut request = agent().post(&url);
        if let Some(token) = token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        read(request.send_json(body).unwrap())
    })
    .await
    .unwrap()
}

pub async fn get(base: &str, path: &str, token: Option<&str>) -> (u16, Value) {
    let url = format!("{base}{path}");
    let token = token.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let mut request = agent().get(&url);
        if let Some(token) = token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        read(request.call().unwrap())
    })
    .await
    .unwrap()
}

/// Fetches a fresh ticket per connection attempt, as the daemon does.
pub struct TicketAuth {
    pub base: String,
    pub ws: String,
    pub token: String,
}

#[async_trait]
impl ConnectAuth for TicketAuth {
    async fn prepare(&self) -> Result<ConnectTarget, AuthError> {
        let (status, body) = post(&self.base, "/api/ws-ticket", Some(&self.token), json!({})).await;
        match status {
            200 => Ok(ConnectTarget {
                url: format!("{}?ticket={}", self.ws, body["ticket"].as_str().unwrap()),
                headers: Vec::new(),
            }),
            401 => Err(AuthError::Rejected("login required".into())),
            other => Err(AuthError::Retry(format!("ticket request: {other}"))),
        }
    }
}

pub struct Client {
    pub repo: Repo,
    pub transport: Arc<WsJsClient>,
}

impl Client {
    pub async fn connect(server: &TestServer, login: &Login, peer: &str) -> Self {
        let mut config = WsJsClientConfig::new(server.ws(), peer).auth(Arc::new(TicketAuth {
            base: server.base(),
            ws: server.ws(),
            token: login.token.clone(),
        }));
        config.min_backoff = Duration::from_millis(50);
        config.max_backoff = Duration::from_millis(200);
        let transport = WsJsClient::start(config);
        let repo = Repo::open(
            Arc::new(MemoryStore::default()),
            Arc::new(MemoryStore::default()),
            transport.clone(),
            RepoConfig::default(),
        )
        .await
        .unwrap();
        let mut peers = repo.subscribe_peers();
        timeout(
            Duration::from_secs(10),
            peers.wait_for(|peers| !peers.is_empty()),
        )
        .await
        .expect("client connects within 10 s")
        .unwrap();
        Self { repo, transport }
    }

    pub async fn find_ready(&self, id: &str) -> DocHandle {
        let handle = self
            .repo
            .find(DocumentId::parse_any(id).unwrap())
            .await
            .unwrap();
        timeout(Duration::from_secs(10), handle.ready())
            .await
            .expect("document arrives within 10 s")
            .unwrap();
        handle
    }

    pub async fn wait_state(&self, what: &str, check: impl Fn(&ConnectionState) -> bool) {
        let mut state = self.transport.subscribe_state();
        timeout(
            Duration::from_secs(10),
            state.wait_for(|state| check(state)),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "client state never became {what}: {:?}",
                self.transport.state()
            )
        })
        .unwrap();
    }
}

/// Polls `check` until true or panics after `seconds`.
pub async fn eventually<F, Fut>(what: &str, seconds: u64, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}
