//! [`ServerTransport`]: the repository's one transport, merging the client
//! websocket server ([`WsJsServer`]) with peer links to member servers, in
//! the pattern of the daemon's `SyncTransport`.
//!
//! Peer links are driven by the peering code (which owns the sockets and the
//! handshakes); this type only keys them by `srv:<server_id>`, taken from
//! the key proven in the TLS handshake, and turns them into repository
//! events. A second link to the same server replaces the first (last link
//! wins). Frames for client peers go to the websocket server.

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
use tokio::sync::{Notify, mpsc, watch};

use crate::access::{server_of, server_peer};

const EVENT_CAPACITY: usize = 1024;
const LINK_QUEUE: usize = 256;

struct Link {
    generation: u64,
    outgoing: mpsc::Sender<Vec<u8>>,
    close: Arc<Notify>,
}

/// One attached peer link, as the code driving its socket sees it.
pub struct Attached {
    pub peer: PeerId,
    pub generation: u64,
    /// Frames the repository sends to the peer.
    pub outgoing: mpsc::Receiver<Vec<u8>>,
    /// Notified when the link must close (replaced, revoked, shutdown).
    pub close: Arc<Notify>,
}

pub struct ServerTransport {
    local: PeerId,
    clients: Arc<WsJsServer>,
    events_tx: mpsc::Sender<NetworkEvent>,
    events: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    links: Mutex<HashMap<String, Link>>,
    linked: watch::Sender<Vec<String>>,
    generation: AtomicU64,
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

    /// Registers a link to `server_id` whose handshake is complete,
    /// replacing any previous link to it, and tells the repository.
    pub async fn attach(&self, server_id: &str) -> Attached {
        let peer = server_peer(server_id);
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let (outgoing_tx, outgoing) = mpsc::channel(LINK_QUEUE);
        let close = Arc::new(Notify::new());
        let replaced = {
            let mut links = self.links.lock().unwrap();
            let replaced = links.insert(
                server_id.to_owned(),
                Link {
                    generation,
                    outgoing: outgoing_tx,
                    close: close.clone(),
                },
            );
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
        Attached {
            peer,
            generation,
            outgoing,
            close,
        }
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
    async fn two_members_link_at_once_and_the_last_link_wins() {
        let transport = ServerTransport::new("aaaa");
        let mut events = transport.take_events().unwrap();
        let mut b = transport.attach("bbbb").await;
        let mut c = transport.attach("cccc").await;
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
        let b2 = transport.attach("bbbb").await;
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
}
