//! Authenticated, reliable, ordered complete-frame transport port.
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{PeerId, error::NetworkError};

/// Ordered lifecycle and complete-frame events from authenticated sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkEvent {
    PeerConnected(PeerId),
    Message { peer: PeerId, bytes: Bytes },
    PeerDisconnected(PeerId),
}

/// Reliable ordered transport. Implementations permit one active session per peer
/// and apply bounded backpressure to `send`. Handshakes (`join`/`peer`) are the
/// transport's job: `PeerConnected` is emitted only after a completed
/// handshake, and `Message` frames carry repository messages only.
#[async_trait]
pub trait NetworkTransport: Send + Sync + 'static {
    /// The id this side uses as `senderId` on the wire.
    fn local_peer(&self) -> PeerId;
    fn take_events(&self) -> Result<mpsc::Receiver<NetworkEvent>, NetworkError>;
    async fn send(&self, peer: &PeerId, frame: Bytes) -> Result<(), NetworkError>;
    async fn close_peer(&self, peer: &PeerId) -> Result<(), NetworkError>;
    async fn close(&self) -> Result<(), NetworkError>;
}
