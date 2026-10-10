//! [`ServerTransport`]: the repository's one transport, merging the client
//! websocket server ([`WsJsServer`]) with peer links to member servers, in
//! the pattern of the daemon's `SyncTransport`.
//!
//! Peer links are driven by the peering code (which owns the sockets and the
//! handshakes); this type only keys them by `srv:<server_id>`, taken from
//! the key proven in the TLS handshake, and turns them into repository
//! events. When two links to the same server exist (both sides dialed at
//! once), the one dialed by the lower server id is kept: a link that is not
//! preferred never replaces one that is, and otherwise the newer link wins.
//! Frames for client peers go to the websocket server.
//!
//! It also keeps the status of every member it linked with: whether a link
//! is open, over which address, when it was last seen, and the last error.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use automerge_repo::{
    PeerId,
    error::NetworkError,
    network::{NetworkEvent, NetworkTransport},
    transport::WsJsServer,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, mpsc, watch};

use crate::{
    access::{server_of, server_peer},
    db::now_ms,
};

const EVENT_CAPACITY: usize = 1024;
const LINK_QUEUE: usize = 256;

struct Link {
    generation: u64,
    /// Dialed by the lower server id of the pair.
    preferred: bool,
    outgoing: mpsc::Sender<Vec<u8>>,
    close: Arc<Notify>,
    probe: Arc<Notify>,
}

/// A member's link state as `peer ls` and `/api/peers` report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerState {
    Online,
    Offline,
    /// Linked, with documents not yet in sync.
    Syncing,
    /// Not linked, and the last attempt failed past connecting (TLS,
    /// hello, protocol) or the link was closed for a reason.
    Error,
}

/// What the transport knows about one member.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkHealth {
    pub linked: bool,
    /// The address of the current or last link.
    pub address: Option<String>,
    /// Unix milliseconds the member was last linked.
    pub last_seen: Option<i64>,
    pub error: Option<String>,
}

impl LinkHealth {
    /// The reported state, given how many documents are not yet in sync
    /// with the member.
    #[must_use]
    pub fn state(&self, pending: usize) -> PeerState {
        match (self.linked, &self.error) {
            (true, _) if pending > 0 => PeerState::Syncing,
            (true, _) => PeerState::Online,
            (false, Some(_)) => PeerState::Error,
            (false, None) => PeerState::Offline,
        }
    }
}

/// One attached peer link, as the code driving its socket sees it.
pub struct Attached {
    pub peer: PeerId,
    pub generation: u64,
    /// Frames the repository sends to the peer.
    pub outgoing: mpsc::Receiver<Vec<u8>>,
    /// Notified when the link must close (replaced, revoked, shutdown).
    pub close: Arc<Notify>,
    /// Notified when a duplicate link was refused in favor of this one:
    /// the link should prove it is still alive.
    pub probe: Arc<Notify>,
}

pub struct ServerTransport {
    local: PeerId,
    clients: Arc<WsJsServer>,
    events_tx: mpsc::Sender<NetworkEvent>,
    events: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    links: Mutex<HashMap<String, Link>>,
    linked: watch::Sender<Vec<String>>,
    generation: AtomicU64,
    health: Mutex<HashMap<String, LinkHealth>>,
}

impl ServerTransport {
    /// A transport announcing itself as `srv:<server_id>` to clients and
    /// peers alike.
    #[must_use]
    pub fn new(server_id: &str) -> Arc<Self> {
        let local = server_peer(server_id);
        let clients = WsJsServer::new(local.clone());
        let (events_tx, events) = mpsc::channel(EVENT_CAPACITY);
        let mut client_events = clients
            .take_events()
            .expect("a fresh websocket server has its events");
        let forward = events_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = client_events.recv().await {
                if forward.send(event).await.is_err() {
                    return;
                }
            }
        });
        Arc::new(Self {
            local,
            clients,
            events_tx,
            events: Mutex::new(Some(events)),
            links: Mutex::new(HashMap::new()),
            linked: watch::Sender::new(Vec::new()),
            generation: AtomicU64::new(0),
            health: Mutex::new(HashMap::new()),
        })
    }

    /// The client websocket server.
    #[must_use]
    pub fn clients(&self) -> &Arc<WsJsServer> {
        &self.clients
    }

    fn publish(&self, links: &HashMap<String, Link>) {
        let mut ids: Vec<String> = links.keys().cloned().collect();
        ids.sort();
        self.linked.send_replace(ids);
    }

    /// Registers a link to `server_id` whose handshake is complete and
    /// tells the repository. `preferred` is whether the link was dialed by
    /// the lower server id; such a link replaces any other, and one that is
    /// not preferred replaces only another one that is not. `None` when the
    /// link is refused as a duplicate (the kept link is asked to probe).
    pub async fn attach(
        &self,
        server_id: &str,
        preferred: bool,
        address: Option<String>,
    ) -> Option<Attached> {
        let peer = server_peer(server_id);
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let (outgoing_tx, outgoing) = mpsc::channel(LINK_QUEUE);
        let close = Arc::new(Notify::new());
        let probe = Arc::new(Notify::new());
        let replaced = {
            let mut links = self.links.lock().unwrap();
            if let Some(kept) = links
                .get(server_id)
                .filter(|existing| existing.preferred && !preferred)
            {
                kept.probe.notify_one();
                return None;
            }
            let replaced = links.insert(
                server_id.to_owned(),
                Link {
                    generation,
                    preferred,
                    outgoing: outgoing_tx,
                    close: close.clone(),
                    probe: probe.clone(),
                },
            );
            let mut health = self.health.lock().unwrap();
            let entry = health.entry(server_id.to_owned()).or_default();
            entry.linked = true;
            entry.error = None;
            entry.last_seen = Some(now_ms());
            if address.is_some() {
                entry.address = address;
            }
            drop(health);
            self.publish(&links);
            replaced
        };
        if let Some(previous) = replaced {
            previous.close.notify_one();
            let _ = self
                .events_tx
                .send(NetworkEvent::PeerDisconnected(peer.clone()))
                .await;
        }
        let _ = self
            .events_tx
            .send(NetworkEvent::PeerConnected(peer.clone()))
            .await;
        Some(Attached {
            peer,
            generation,
            outgoing,
            close,
            probe,
        })
    }

    /// Hands a frame received on a link to the repository.
    pub async fn deliver(&self, peer: &PeerId, frame: Vec<u8>) {
        let _ = self
            .events_tx
            .send(NetworkEvent::Message {
                peer: peer.clone(),
                bytes: Bytes::from(frame),
            })
            .await;
    }

    /// Unregisters a link that ended; a link already replaced is left alone.
    pub async fn detach(&self, server_id: &str, generation: u64) {
        let current = {
            let mut links = self.links.lock().unwrap();
            let current = links
                .get(server_id)
                .is_some_and(|link| link.generation == generation);
            if current {
                links.remove(server_id);
                let mut health = self.health.lock().unwrap();
                let entry = health.entry(server_id.to_owned()).or_default();
                entry.linked = false;
                entry.last_seen = Some(now_ms());
                drop(health);
                self.publish(&links);
            }
            current
        };
        if current {
            let _ = self
                .events_tx
                .send(NetworkEvent::PeerDisconnected(server_peer(server_id)))
                .await;
        }
    }

    /// Records why a link with `server_id` failed or closed; `None` clears
    /// the error (a plain connection failure: the member is offline).
    /// Ignored while a link is open.
    pub fn record_error(&self, server_id: &str, error: Option<String>) {
        let linked = self.links.lock().unwrap().contains_key(server_id);
        if linked {
            return;
        }
        self.health
            .lock()
            .unwrap()
            .entry(server_id.to_owned())
            .or_default()
            .error = error;
    }

    /// What is known about every member linked since the start.
    #[must_use]
    pub fn health(&self) -> HashMap<String, LinkHealth> {
        self.health.lock().unwrap().clone()
    }

    /// Closes the link to `server_id`, if any.
    pub fn close_link(&self, server_id: &str) {
        if let Some(link) = self.links.lock().unwrap().get(server_id) {
            link.close.notify_one();
        }
    }

    /// Server ids with an open link, sorted.
    #[must_use]
    pub fn linked(&self) -> Vec<String> {
        self.linked.borrow().clone()
    }

    /// Watches the set of linked server ids.
    #[must_use]
    pub fn subscribe_links(&self) -> watch::Receiver<Vec<String>> {
        self.linked.subscribe()
    }
}

#[async_trait]
impl NetworkTransport for ServerTransport {
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
        let Some(server) = server_of(peer) else {
            return self.clients.send(peer, frame).await;
        };
        let sender = match self.links.lock().unwrap().get(server) {
            Some(link) => link.outgoing.clone(),
            None => return Err(NetworkError::NotConnected(peer.clone())),
        };
        sender
            .send(frame.to_vec())
            .await
            .map_err(|_| NetworkError::NotConnected(peer.clone()))
    }
    async fn close_peer(&self, peer: &PeerId) -> Result<(), NetworkError> {
        match server_of(peer) {
            Some(server) => {
                self.close_link(server);
                Ok(())
            }
            None => self.clients.close_peer(peer).await,
        }
    }
    async fn close(&self) -> Result<(), NetworkError> {
        for link in self.links.lock().unwrap().values() {
            link.close.notify_one();
        }
        self.clients.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn next(events: &mut mpsc::Receiver<NetworkEvent>) -> NetworkEvent {
        tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn two_members_link_at_once_and_an_equal_link_replaces_the_first() {
        let transport = ServerTransport::new("aaaa");
        let mut events = transport.take_events().unwrap();
        let mut b = transport.attach("bbbb", true, None).await.unwrap();
        let mut c = transport.attach("cccc", true, None).await.unwrap();
        assert_eq!(
            next(&mut events).await,
            NetworkEvent::PeerConnected(server_peer("bbbb"))
        );
        assert_eq!(
            next(&mut events).await,
            NetworkEvent::PeerConnected(server_peer("cccc"))
        );
        assert_eq!(transport.linked(), ["bbbb", "cccc"]);

        // Each link gets its own frames.
        transport
            .send(&server_peer("bbbb"), Bytes::from_static(b"to b"))
            .await
            .unwrap();
        transport
            .send(&server_peer("cccc"), Bytes::from_static(b"to c"))
            .await
            .unwrap();
        assert_eq!(b.outgoing.recv().await.unwrap(), b"to b");
        assert_eq!(c.outgoing.recv().await.unwrap(), b"to c");
        transport.deliver(&c.peer, b"from c".to_vec()).await;
        assert_eq!(
            next(&mut events).await,
            NetworkEvent::Message {
                peer: server_peer("cccc"),
                bytes: Bytes::from_static(b"from c")
            }
        );

        // A second link to B replaces the first, which is told to close.
        let b2 = transport.attach("bbbb", true, None).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), b.close.notified())
            .await
            .unwrap();
        assert_eq!(
            next(&mut events).await,
            NetworkEvent::PeerDisconnected(server_peer("bbbb"))
        );
        assert_eq!(
            next(&mut events).await,
            NetworkEvent::PeerConnected(server_peer("bbbb"))
        );
        // The replaced link ending does not disconnect the new one.
        transport.detach("bbbb", b.generation).await;
        assert_eq!(transport.linked(), ["bbbb", "cccc"]);
        transport.detach("bbbb", b2.generation).await;
        assert_eq!(
            next(&mut events).await,
            NetworkEvent::PeerDisconnected(server_peer("bbbb"))
        );
        assert_eq!(transport.linked(), ["cccc"]);
        assert!(
            transport
                .send(&server_peer("bbbb"), Bytes::from_static(b"x"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_link_dialed_by_the_lower_id_is_never_replaced_by_the_other() {
        let transport = ServerTransport::new("aaaa");
        let mut events = transport.take_events().unwrap();
        let kept = transport.attach("bbbb", true, None).await.unwrap();
        next(&mut events).await;
        assert!(transport.attach("bbbb", false, None).await.is_none());
        tokio::time::timeout(std::time::Duration::from_secs(5), kept.probe.notified())
            .await
            .expect("the kept link is asked to probe");
        assert_eq!(transport.linked(), ["bbbb"]);
        // The other way round, the preferred link replaces the first.
        let other = transport.attach("cccc", false, None).await.unwrap();
        next(&mut events).await;
        let preferred = transport.attach("cccc", true, None).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), other.close.notified())
            .await
            .expect("the duplicate is told to close");
        transport.detach("cccc", other.generation).await;
        assert_eq!(transport.linked(), ["bbbb", "cccc"]);
        transport.detach("cccc", preferred.generation).await;
        assert_eq!(transport.linked(), ["bbbb"]);
    }

    #[tokio::test]
    async fn member_state_follows_links_errors_and_pending_documents() {
        let transport = ServerTransport::new("aaaa");
        let _events = transport.take_events().unwrap();
        assert!(!transport.health().contains_key("bbbb"));

        // A failed attempt before any link.
        transport.record_error("bbbb", Some("TLS handshake failed".into()));
        let health = transport.health()["bbbb"].clone();
        assert_eq!(health.state(0), PeerState::Error);
        assert_eq!(health.last_seen, None);
        transport.record_error("bbbb", None);
        assert_eq!(transport.health()["bbbb"].state(0), PeerState::Offline);

        // Linked: online, or syncing while documents are pending.
        let link = transport
            .attach("bbbb", true, Some("10.0.0.2:8772".into()))
            .await
            .unwrap();
        let health = transport.health()["bbbb"].clone();
        assert_eq!(health.state(0), PeerState::Online);
        assert_eq!(health.state(3), PeerState::Syncing);
        assert_eq!(health.address.as_deref(), Some("10.0.0.2:8772"));
        let linked_at = health.last_seen.unwrap();
        // Errors are not recorded over an open link.
        transport.record_error("bbbb", Some("ignored".into()));
        assert_eq!(transport.health()["bbbb"].state(0), PeerState::Online);

        // Unlinked: offline, keeping the address and the last-seen time.
        transport.detach("bbbb", link.generation).await;
        let health = transport.health()["bbbb"].clone();
        assert_eq!(health.state(5), PeerState::Offline);
        assert_eq!(health.address.as_deref(), Some("10.0.0.2:8772"));
        assert!(health.last_seen.unwrap() >= linked_at);
        transport.record_error("bbbb", Some("closed: protocol error".into()));
        assert_eq!(transport.health()["bbbb"].state(0), PeerState::Error);

        // A new link clears the error.
        transport.attach("bbbb", false, None).await.unwrap();
        let health = transport.health()["bbbb"].clone();
        assert_eq!(health.state(0), PeerState::Online);
        assert_eq!(health.error, None);
        assert_eq!(health.address.as_deref(), Some("10.0.0.2:8772"));
    }
}
