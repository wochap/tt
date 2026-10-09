//! Switchable transport: the Repo is opened once, while the websocket client
//! behind it can be started, replaced (after `tt login`) or stopped. With no
//! server configured no client exists and nothing touches the network.
//!
//! Each connection attempt first asks the server for a single-use websocket
//! ticket (`POST /api/ws-ticket` with the bearer token), so the long-lived
//! token never appears in a URL. A 401 means the token was revoked: the
//! client stops and `tt status` reports that login is required. A server
//! without the ticket endpoint (404, e.g. a plain automerge-repo sync server)
//! gets the token as an `Authorization` header instead.
//!
//! Every connection (ticket request and `wss://`) trusts the webpki roots
//! plus `server.ca_cert`. An invalid CA file starts no client: status reports
//! `ca_cert_invalid` instead of silently falling back to webpki-only trust.
//!
//! [`SyncScope`] is the client-side access policy: only documents reachable
//! from this device's index are announced to or accepted from the server, so
//! stale local documents (an offline workspace from before `tt login`) are
//! never pushed.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use automerge_repo::{
    AccessPolicy, DocumentId, PeerId,
    error::NetworkError,
    network::{NetworkEvent, NetworkTransport},
    transport::{
        AuthError, ConnectAuth, ConnectTarget, ConnectionState, WsJsClient, WsJsClientConfig,
    },
};
use bytes::Bytes;
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::config::{InvalidCaCert, SyncEndpoint};

/// Documents this device syncs: its index and what the index lists.
#[derive(Debug, Default)]
pub struct SyncScope {
    ids: RwLock<HashSet<DocumentId>>,
}

impl SyncScope {
    /// Forgets everything (the index changed).
    pub fn reset(&self) {
        self.ids.write().unwrap().clear();
    }
    /// Adds `id`; `true` when it was new.
    pub fn insert(&self, id: DocumentId) -> bool {
        self.ids.write().unwrap().insert(id)
    }
    #[must_use]
    pub fn contains(&self, id: DocumentId) -> bool {
        self.ids.read().unwrap().contains(&id)
    }
}

/// [`AccessPolicy`] backed by a [`SyncScope`].
pub struct ScopePolicy(pub Arc<SyncScope>);

impl AccessPolicy for ScopePolicy {
    fn may_sync(&self, _peer: &PeerId, document: DocumentId) -> bool {
        self.0.contains(document)
    }
}

enum TicketReply {
    Ticket(String),
    NoTicketEndpoint,
    Unauthorized,
    Failed(String),
}

fn request_ticket(endpoint: &SyncEndpoint) -> TicketReply {
    let agent = crate::tls::ureq_agent(&endpoint.extra_roots, Duration::from_secs(10));
    let response = agent
        .post(&format!("{}/api/ws-ticket", endpoint.base))
        .header("Authorization", &format!("Bearer {}", endpoint.token))
        .send_json(json!({}));
    let mut response = match response {
        Ok(response) => response,
        Err(error) => return TicketReply::Failed(format!("ticket request: {error}")),
    };
    match response.status().as_u16() {
        200 => match response.body_mut().read_json::<Value>() {
            Ok(body) => match body["ticket"].as_str() {
                Some(ticket) => TicketReply::Ticket(ticket.to_owned()),
                None => TicketReply::Failed("ticket response has no ticket".into()),
            },
            Err(error) => TicketReply::Failed(format!("ticket response: {error}")),
        },
        401 | 403 => TicketReply::Unauthorized,
        404 | 405 => TicketReply::NoTicketEndpoint,
        status => TicketReply::Failed(format!("ticket request: HTTP {status}")),
    }
}

struct TicketAuth(SyncEndpoint);

#[async_trait]
impl ConnectAuth for TicketAuth {
    async fn prepare(&self) -> Result<ConnectTarget, AuthError> {
        let endpoint = self.0.clone();
        let reply = tokio::task::spawn_blocking(move || request_ticket(&endpoint))
            .await
            .map_err(|error| AuthError::Retry(error.to_string()))?;
        match reply {
            TicketReply::Ticket(ticket) => Ok(ConnectTarget {
                url: format!("{}?ticket={ticket}", self.0.websocket),
                headers: Vec::new(),
            }),
            TicketReply::NoTicketEndpoint => Ok(ConnectTarget {
                url: self.0.websocket.clone(),
                headers: vec![("Authorization".into(), format!("Bearer {}", self.0.token))],
            }),
            TicketReply::Unauthorized => Err(AuthError::Rejected(
                "login required: the server refused the token (run `tt login`)".into(),
            )),
            TicketReply::Failed(message) => Err(AuthError::Retry(message)),
        }
    }
}

struct Active {
    url: String,
    client: Arc<WsJsClient>,
    forwarder: JoinHandle<()>,
}

pub struct SyncTransport {
    local: PeerId,
    events_tx: mpsc::Sender<NetworkEvent>,
    events: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    active: Mutex<Option<Active>>,
    /// Set while `server.ca_cert` is invalid (no client runs then).
    invalid_ca: Mutex<Option<InvalidCaCert>>,
}

impl SyncTransport {
    #[must_use]
    pub fn new(local: PeerId) -> Arc<Self> {
        let (events_tx, events) = mpsc::channel(1024);
        Arc::new(Self {
            local,
            events_tx,
            events: Mutex::new(Some(events)),
            active: Mutex::new(None),
            invalid_ca: Mutex::new(None),
        })
    }

    /// Starts (or restarts) the client for `endpoint`.
    pub async fn connect(&self, endpoint: SyncEndpoint) {
        self.disconnect().await;
        let url = endpoint.base.clone();
        let config = WsJsClientConfig::new(endpoint.websocket.clone(), self.local.clone())
            .tls(crate::tls::rustls_client_config(&endpoint.extra_roots))
            .auth(Arc::new(TicketAuth(endpoint)));
        let client = WsJsClient::start(config);
        let mut events = client
            .take_events()
            .expect("fresh client has its event receiver");
        let forward = self.events_tx.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if forward.send(event).await.is_err() {
                    break;
                }
            }
        });
        *self.active.lock().unwrap() = Some(Active {
            url,
            client,
            forwarder,
        });
    }

    /// Applies `Config::sync_endpoint`: connect, stop, or (invalid CA) stop
    /// and report `ca_cert_invalid`.
    pub async fn apply(&self, endpoint: Result<Option<SyncEndpoint>, InvalidCaCert>) {
        match endpoint {
            Ok(Some(endpoint)) => self.connect(endpoint).await,
            Ok(None) => self.disconnect().await,
            Err(invalid) => {
                self.disconnect().await;
                *self.invalid_ca.lock().unwrap() = Some(invalid);
            }
        }
    }

    /// Stops the client, reporting its peer as disconnected.
    pub async fn disconnect(&self) {
        self.invalid_ca.lock().unwrap().take();
        let previous = self.active.lock().unwrap().take();
        if let Some(active) = previous {
            let remote = match active.client.state() {
                ConnectionState::Connected { remote } => Some(remote),
                _ => None,
            };
            let _ = active.client.close().await;
            active.forwarder.abort();
            if let Some(remote) = remote {
                let _ = self
                    .events_tx
                    .send(NetworkEvent::PeerDisconnected(remote))
                    .await;
            }
        }
    }

    /// Status for `tt status`.
    #[must_use]
    pub fn status(&self) -> Value {
        if let Some(invalid) = self.invalid_ca.lock().unwrap().as_ref() {
            return json!({
                "configured": true,
                "state": "ca_cert_invalid",
                "detail": {"path": invalid.path, "error": invalid.message},
            });
        }
        let active = self.active.lock().unwrap();
        let Some(active) = active.as_ref() else {
            return json!({"configured": false, "state": "offline"});
        };
        let (state, detail) = match active.client.state() {
            ConnectionState::Connecting => ("connecting", Value::Null),
            ConnectionState::Connected { remote } => ("connected", json!(remote.to_string())),
            ConnectionState::Disconnected { error, retry_in } => (
                "disconnected",
                json!({"error": error, "retry_in_ms": retry_in.as_millis() as u64}),
            ),
            ConnectionState::Failed(error) => ("failed", json!(error.to_string())),
            ConnectionState::Rejected(message) => ("login_required", json!(message)),
            ConnectionState::Closed => ("closed", Value::Null),
        };
        json!({"configured": true, "url": active.url, "state": state, "detail": detail})
    }
}

#[async_trait]
impl NetworkTransport for SyncTransport {
    fn local_peer(&self) -> PeerId {
        self.local.clone()
    }
    fn take_events(&self) -> Result<mpsc::Receiver<NetworkEvent>, NetworkError> {
        self.events
            .lock()
            .unwrap()
            .take()
            .ok_or(NetworkError::EventsAlreadyTaken)
    }
    async fn send(&self, peer: &PeerId, frame: Bytes) -> Result<(), NetworkError> {
        let client = self
            .active
            .lock()
            .unwrap()
            .as_ref()
            .map(|active| active.client.clone());
        match client {
            Some(client) => client.send(peer, frame).await,
            None => Err(NetworkError::NotConnected(peer.clone())),
        }
    }
    async fn close_peer(&self, peer: &PeerId) -> Result<(), NetworkError> {
        let client = self
            .active
            .lock()
            .unwrap()
            .as_ref()
            .map(|active| active.client.clone());
        match client {
            Some(client) => client.close_peer(peer).await,
            None => Ok(()),
        }
    }
    async fn close(&self) -> Result<(), NetworkError> {
        self.disconnect().await;
        Ok(())
    }
}
