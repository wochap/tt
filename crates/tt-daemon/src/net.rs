//! Switchable transport: the Repo is opened once, while the websocket client
//! behind it can be started, replaced (after `tt login`) or stopped. With no
//! server configured no client exists and nothing touches the network.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use automerge_repo::{
    PeerId,
    error::NetworkError,
    network::{NetworkEvent, NetworkTransport},
    transport::{ConnectionState, WsJsClient, WsJsClientConfig},
};
use bytes::Bytes;
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};

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
        })
    }

    /// Starts (or restarts) the client for `url` with a bearer token.
    pub async fn connect(&self, url: &str, token: &str) {
        self.disconnect().await;
        let config = WsJsClientConfig::new(url, self.local.clone()).bearer(token);
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
            url: url.to_owned(),
            client,
            forwarder,
        });
    }

    /// Stops the client, reporting its peer as disconnected.
    pub async fn disconnect(&self) {
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
