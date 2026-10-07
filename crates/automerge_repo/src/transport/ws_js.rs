//! Websocket transports speaking the automerge-repo JS protocol.
//!
//! [`WsJsClient`] dials one server (for example
//! `@automerge/automerge-repo-sync-server`), performs the `join`/`peer`
//! handshake and reconnects with exponential backoff. [`WsJsServer`] accepts
//! many connections and runs the same handshake from the server side; it is
//! socket-agnostic so an HTTP framework can hand it upgraded connections.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{Notify, mpsc, watch},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message as WsMessage, client::IntoClientRequest, http::HeaderValue},
};

use crate::{
    PeerId,
    error::{NetworkError, ProtocolError},
    network::{NetworkEvent, NetworkTransport},
    protocol::{PROTOCOL_VERSION, PeerMetadata, WireMessage, retarget},
};

const EVENT_CAPACITY: usize = 1024;
const OUTGOING_CAPACITY: usize = 256;

/// Observable state of a client connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Connecting,
    Connected {
        remote: PeerId,
    },
    /// The last attempt failed or the connection dropped; a retry is scheduled.
    Disconnected {
        error: String,
        retry_in: Duration,
    },
    /// A non-retryable failure, such as a protocol version mismatch.
    Failed(ProtocolError),
    /// [`ConnectAuth::prepare`] reported that the credentials were refused;
    /// not retried.
    Rejected(String),
    Closed,
}

/// Where and how to dial for one connection attempt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectTarget {
    /// `ws://` or `wss://` URL, e.g. with a single-use ticket in the query.
    pub url: String,
    /// Extra HTTP headers for the upgrade request.
    pub headers: Vec<(String, String)>,
}

/// Why [`ConnectAuth::prepare`] could not produce a target.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthError {
    /// Transient (server unreachable, 5xx): retried with backoff.
    #[error("{0}")]
    Retry(String),
    /// Credentials refused (401): the client stops in
    /// [`ConnectionState::Rejected`].
    #[error("{0}")]
    Rejected(String),
}

/// Per-attempt connection setup, e.g. fetching a short-lived websocket ticket
/// so a long-lived token never appears in a URL.
#[async_trait]
pub trait ConnectAuth: Send + Sync + 'static {
    async fn prepare(&self) -> Result<ConnectTarget, AuthError>;
}

#[derive(Clone)]
pub struct WsJsClientConfig {
    /// `ws://` or `wss://` URL.
    pub url: String,
    /// Our `senderId`.
    pub peer_id: PeerId,
    /// Extra HTTP headers for the upgrade request, e.g. `Authorization`.
    pub headers: Vec<(String, String)>,
    pub peer_metadata: PeerMetadata,
    pub handshake_timeout: Duration,
    pub min_backoff: Duration,
    pub max_backoff: Duration,
    /// Called before every attempt; its target replaces `url` and adds
    /// headers. `None` dials `url` directly.
    pub auth: Option<Arc<dyn ConnectAuth>>,
}

impl std::fmt::Debug for WsJsClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsJsClientConfig")
            .field("url", &self.url)
            .field("peer_id", &self.peer_id)
            .field("headers", &self.headers.len())
            .field("auth", &self.auth.is_some())
            .finish_non_exhaustive()
    }
}

impl WsJsClientConfig {
    #[must_use]
    pub fn new(url: impl Into<String>, peer_id: impl Into<PeerId>) -> Self {
        Self {
            url: url.into(),
            peer_id: peer_id.into(),
            headers: Vec::new(),
            peer_metadata: PeerMetadata {
                storage_id: None,
                is_ephemeral: Some(false),
            },
            handshake_timeout: Duration::from_secs(10),
            min_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(30),
            auth: None,
        }
    }
    #[must_use]
    pub fn auth(mut self, auth: Arc<dyn ConnectAuth>) -> Self {
        self.auth = Some(auth);
        self
    }
    #[must_use]
    pub fn bearer(mut self, token: &str) -> Self {
        self.headers
            .push(("Authorization".into(), format!("Bearer {token}")));
        self
    }
}

#[derive(Debug, thiserror::Error)]
enum AttemptError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Rejected(String),
}

struct ClientShared {
    config: WsJsClientConfig,
    events: mpsc::Sender<NetworkEvent>,
    current: Mutex<Option<(PeerId, mpsc::Sender<Vec<u8>>)>>,
    state: watch::Sender<ConnectionState>,
    closed: AtomicBool,
    shutdown: Notify,
    disconnect: Notify,
}

/// Reconnecting websocket client transport.
pub struct WsJsClient {
    shared: Arc<ClientShared>,
    events: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    state: watch::Receiver<ConnectionState>,
}

impl WsJsClient {
    /// Starts the connection loop in the background. The first attempt is made
    /// immediately; failures are retried with exponential backoff except a
    /// protocol version mismatch, which ends in [`ConnectionState::Failed`].
    #[must_use]
    pub fn start(config: WsJsClientConfig) -> Arc<Self> {
        let (events_tx, events_rx) = mpsc::channel(EVENT_CAPACITY);
        let (state_tx, state) = watch::channel(ConnectionState::Connecting);
        let shared = Arc::new(ClientShared {
            config,
            events: events_tx,
            current: Mutex::new(None),
            state: state_tx,
            closed: AtomicBool::new(false),
            shutdown: Notify::new(),
            disconnect: Notify::new(),
        });
        tokio::spawn(run_client(shared.clone()));
        Arc::new(Self {
            shared,
            events: Mutex::new(Some(events_rx)),
            state,
        })
    }

    #[must_use]
    pub fn state(&self) -> ConnectionState {
        self.state.borrow().clone()
    }

    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<ConnectionState> {
        self.state.clone()
    }
}

async fn run_client(shared: Arc<ClientShared>) {
    let mut backoff = shared.config.min_backoff;
    while !shared.closed.load(Ordering::Acquire) {
        shared.state.send_replace(ConnectionState::Connecting);
        let started = tokio::time::Instant::now();
        let result = attempt(&shared).await;
        if shared.closed.load(Ordering::Acquire) {
            break;
        }
        let error = match result {
            Ok(()) => "connection closed".to_owned(),
            Err(AttemptError::Protocol(
                error @ (ProtocolError::VersionMismatch { .. } | ProtocolError::Remote(_)),
            )) => {
                shared.state.send_replace(ConnectionState::Failed(error));
                return;
            }
            Err(AttemptError::Rejected(message)) => {
                shared
                    .state
                    .send_replace(ConnectionState::Rejected(message));
                return;
            }
            Err(error) => error.to_string(),
        };
        // A connection that stayed up for a while earns a fresh backoff.
        if started.elapsed() > shared.config.max_backoff {
            backoff = shared.config.min_backoff;
        }
        shared.state.send_replace(ConnectionState::Disconnected {
            error,
            retry_in: backoff,
        });
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            () = shared.shutdown.notified() => break,
        }
        backoff = (backoff * 2).min(shared.config.max_backoff);
    }
    shared.state.send_replace(ConnectionState::Closed);
}

async fn attempt(shared: &Arc<ClientShared>) -> Result<(), AttemptError> {
    // `wss://` uses rustls with webpki roots; pin the ring provider so the
    // choice never depends on which crypto features other crates enable.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = &shared.config;
    let target = match &config.auth {
        Some(auth) => auth.prepare().await.map_err(|error| match error {
            AuthError::Retry(message) => AttemptError::Io(message),
            AuthError::Rejected(message) => AttemptError::Rejected(message),
        })?,
        None => ConnectTarget {
            url: config.url.clone(),
            headers: Vec::new(),
        },
    };
    let mut request = target
        .url
        .as_str()
        .into_client_request()
        .map_err(|error| AttemptError::Io(error.to_string()))?;
    for (name, value) in config.headers.iter().chain(&target.headers) {
        let value =
            HeaderValue::from_str(value).map_err(|error| AttemptError::Io(error.to_string()))?;
        let name: tokio_tungstenite::tungstenite::http::HeaderName = name
            .parse()
            .map_err(|_| AttemptError::Io(format!("invalid header name {name}")))?;
        request.headers_mut().insert(name, value);
    }
    let (mut socket, _) = tokio::time::timeout(
        config.handshake_timeout,
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| AttemptError::Io("connect timed out".into()))?
    .map_err(|error| AttemptError::Io(error.to_string()))?;
    let join = WireMessage::Join {
        sender_id: config.peer_id.to_string(),
        peer_metadata: config.peer_metadata.clone(),
        supported_protocol_versions: vec![PROTOCOL_VERSION.into()],
    };
    socket
        .send(WsMessage::Binary(join.encode()?.into()))
        .await
        .map_err(|error| AttemptError::Io(error.to_string()))?;
    let remote = tokio::time::timeout(config.handshake_timeout, async {
        loop {
            let Some(frame) = socket.next().await else {
                return Err(AttemptError::Io("closed during handshake".into()));
            };
            let frame = frame.map_err(|error| AttemptError::Io(error.to_string()))?;
            let WsMessage::Binary(bytes) = frame else {
                continue;
            };
            match WireMessage::decode(&bytes)? {
                WireMessage::Peer {
                    sender_id,
                    selected_protocol_version,
                    ..
                } => {
                    if selected_protocol_version != PROTOCOL_VERSION {
                        return Err(ProtocolError::VersionMismatch {
                            selected: selected_protocol_version,
                        }
                        .into());
                    }
                    return Ok(PeerId::from(sender_id));
                }
                WireMessage::Error { message, .. } => {
                    return Err(ProtocolError::Remote(message).into());
                }
                other => {
                    return Err(ProtocolError::UnexpectedHandshake(other.type_name().into()).into());
                }
            }
        }
    })
    .await
    .map_err(|_| AttemptError::Io("handshake timed out".into()));
    let remote = match remote {
        Ok(Ok(remote)) => remote,
        Ok(Err(error)) | Err(error) => {
            let _ = socket.close(None).await;
            return Err(error);
        }
    };
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(OUTGOING_CAPACITY);
    *shared.current.lock().unwrap() = Some((remote.clone(), out_tx));
    shared.state.send_replace(ConnectionState::Connected {
        remote: remote.clone(),
    });
    let _ = shared
        .events
        .send(NetworkEvent::PeerConnected(remote.clone()))
        .await;
    let result = loop {
        tokio::select! {
            frame = socket.next() => match frame {
                Some(Ok(WsMessage::Binary(bytes))) => {
                    let _ = shared.events.send(NetworkEvent::Message {
                        peer: remote.clone(),
                        bytes: Bytes::from(bytes.to_vec()),
                    }).await;
                }
                Some(Ok(WsMessage::Close(_))) | None => break Ok(()),
                Some(Ok(_)) => {}
                Some(Err(error)) => break Err(AttemptError::Io(error.to_string())),
            },
            outgoing = out_rx.recv() => match outgoing {
                Some(frame) => {
                    if let Err(error) = socket.send(WsMessage::Binary(frame.into())).await {
                        break Err(AttemptError::Io(error.to_string()));
                    }
                }
                None => break Ok(()),
            },
            () = shared.disconnect.notified() => {
                let _ = socket.close(None).await;
                break Ok(());
            }
            () = shared.shutdown.notified() => {
                let _ = socket.close(None).await;
                break Ok(());
            }
        }
    };
    *shared.current.lock().unwrap() = None;
    let _ = shared
        .events
        .send(NetworkEvent::PeerDisconnected(remote))
        .await;
    result
}

#[async_trait]
impl NetworkTransport for WsJsClient {
    fn local_peer(&self) -> PeerId {
        self.shared.config.peer_id.clone()
    }
    fn take_events(&self) -> Result<mpsc::Receiver<NetworkEvent>, NetworkError> {
        self.events
            .lock()
            .unwrap()
            .take()
            .ok_or(NetworkError::EventsAlreadyTaken)
    }
    async fn send(&self, peer: &PeerId, frame: Bytes) -> Result<(), NetworkError> {
        let sender = match self.shared.current.lock().unwrap().as_ref() {
            Some((remote, sender)) if remote == peer => sender.clone(),
            _ => return Err(NetworkError::NotConnected(peer.clone())),
        };
        sender
            .send(frame.to_vec())
            .await
            .map_err(|_| NetworkError::NotConnected(peer.clone()))
    }
    async fn close_peer(&self, _peer: &PeerId) -> Result<(), NetworkError> {
        // Drops the current socket; the loop reconnects with a fresh session.
        self.shared.disconnect.notify_waiters();
        Ok(())
    }
    async fn close(&self) -> Result<(), NetworkError> {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.shutdown.notify_waiters();
        Ok(())
    }
}

struct ServerConnection {
    generation: u64,
    wire_id: String,
    outgoing: mpsc::Sender<Vec<u8>>,
    close: Arc<Notify>,
}

/// Server-side transport: one session per accepted connection.
pub struct WsJsServer {
    local: PeerId,
    peer_metadata: PeerMetadata,
    events_tx: mpsc::Sender<NetworkEvent>,
    events: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    connections: Mutex<HashMap<PeerId, ServerConnection>>,
    generation: AtomicU64,
    handshake_timeout: Duration,
}

impl WsJsServer {
    #[must_use]
    pub fn new(local: impl Into<PeerId>) -> Arc<Self> {
        let (events_tx, events) = mpsc::channel(EVENT_CAPACITY);
        Arc::new(Self {
            local: local.into(),
            peer_metadata: PeerMetadata {
                storage_id: None,
                is_ephemeral: Some(false),
            },
            events_tx,
            events: Mutex::new(Some(events)),
            connections: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            handshake_timeout: Duration::from_secs(10),
        })
    }

    /// Accepts websocket connections on `listener` until it fails. Every
    /// connection uses the client's own `senderId` as its peer id.
    pub async fn listen(self: Arc<Self>, listener: TcpListener) -> std::io::Result<()> {
        loop {
            let (stream, _) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Ok(socket) = tokio_tungstenite::accept_async(stream).await {
                    let _ = server.serve_websocket(socket, None).await;
                }
            });
        }
    }

    /// Runs one upgraded tungstenite websocket to completion.
    pub async fn serve_websocket<S>(
        &self,
        socket: WebSocketStream<S>,
        identity: Option<String>,
    ) -> Result<(), ProtocolError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sink, mut stream) = socket.split();
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(OUTGOING_CAPACITY);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(OUTGOING_CAPACITY);
        let reader = tokio::spawn(async move {
            while let Some(Ok(frame)) = stream.next().await {
                let bytes = match frame {
                    WsMessage::Binary(bytes) => bytes,
                    WsMessage::Close(_) => break,
                    _ => continue,
                };
                if in_tx.send(bytes.to_vec()).await.is_err() {
                    break;
                }
            }
        });
        let writer = tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if sink.send(WsMessage::Binary(frame.into())).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let result = self.serve_frames(in_rx, out_tx, identity).await;
        reader.abort();
        let _ = writer.await;
        result
    }

    /// Socket-agnostic session: `incoming` yields binary frames from the
    /// client, `outgoing` accepts frames for it. With `identity` set the
    /// repository-facing peer id is `<identity>/<senderId>` (authenticated
    /// servers); frames are retargeted to the client's wire id on send.
    pub async fn serve_frames(
        &self,
        mut incoming: mpsc::Receiver<Vec<u8>>,
        outgoing: mpsc::Sender<Vec<u8>>,
        identity: Option<String>,
    ) -> Result<(), ProtocolError> {
        let join = tokio::time::timeout(self.handshake_timeout, incoming.recv())
            .await
            .map_err(|_| ProtocolError::Cbor("handshake timed out".into()))?
            .ok_or_else(|| ProtocolError::Cbor("closed before join".into()))?;
        let WireMessage::Join {
            sender_id,
            supported_protocol_versions,
            ..
        } = WireMessage::decode(&join)?
        else {
            return Err(ProtocolError::UnexpectedHandshake("non-join first".into()));
        };
        if !supported_protocol_versions.is_empty()
            && !supported_protocol_versions
                .iter()
                .any(|version| version == PROTOCOL_VERSION)
        {
            let error = WireMessage::Error {
                sender_id: self.local.to_string(),
                target_id: Some(sender_id),
                message: "unsupported protocol version".into(),
            };
            let _ = outgoing.send(error.encode()?).await;
            return Err(ProtocolError::VersionMismatch {
                selected: supported_protocol_versions.join(","),
            });
        }
        let peer = PeerId::from(match &identity {
            Some(identity) => format!("{identity}/{sender_id}"),
            None => sender_id.clone(),
        });
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let close = Arc::new(Notify::new());
        let replaced = self.connections.lock().unwrap().insert(
            peer.clone(),
            ServerConnection {
                generation,
                wire_id: sender_id.clone(),
                outgoing: outgoing.clone(),
                close: close.clone(),
            },
        );
        if let Some(previous) = replaced {
            previous.close.notify_one();
            let _ = self
                .events_tx
                .send(NetworkEvent::PeerDisconnected(peer.clone()))
                .await;
        }
        let reply = WireMessage::Peer {
            sender_id: self.local.to_string(),
            target_id: sender_id,
            selected_protocol_version: PROTOCOL_VERSION.into(),
            peer_metadata: self.peer_metadata.clone(),
        };
        outgoing
            .send(reply.encode()?)
            .await
            .map_err(|_| ProtocolError::Cbor("closed during handshake".into()))?;
        let _ = self
            .events_tx
            .send(NetworkEvent::PeerConnected(peer.clone()))
            .await;
        loop {
            tokio::select! {
                frame = incoming.recv() => match frame {
                    Some(frame) => {
                        let _ = self.events_tx.send(NetworkEvent::Message {
                            peer: peer.clone(),
                            bytes: Bytes::from(frame),
                        }).await;
                    }
                    None => break,
                },
                () = close.notified() => break,
            }
        }
        let current = {
            let mut connections = self.connections.lock().unwrap();
            let current = connections
                .get(&peer)
                .is_some_and(|connection| connection.generation == generation);
            if current {
                connections.remove(&peer);
            }
            current
        };
        if current {
            let _ = self
                .events_tx
                .send(NetworkEvent::PeerDisconnected(peer))
                .await;
        }
        Ok(())
    }
}

#[async_trait]
impl NetworkTransport for WsJsServer {
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
        let (sender, wire_id) = match self.connections.lock().unwrap().get(peer) {
            Some(connection) => (connection.outgoing.clone(), connection.wire_id.clone()),
            None => return Err(NetworkError::NotConnected(peer.clone())),
        };
        let frame = if wire_id == peer.as_str() {
            frame.to_vec()
        } else {
            retarget(&frame, &wire_id).map_err(|error| NetworkError::Transport {
                peer: peer.clone(),
                message: error.to_string(),
            })?
        };
        sender
            .send(frame)
            .await
            .map_err(|_| NetworkError::NotConnected(peer.clone()))
    }
    async fn close_peer(&self, peer: &PeerId) -> Result<(), NetworkError> {
        if let Some(connection) = self.connections.lock().unwrap().get(peer) {
            connection.close.notify_one();
        }
        Ok(())
    }
    async fn close(&self) -> Result<(), NetworkError> {
        for connection in self.connections.lock().unwrap().values() {
            connection.close.notify_one();
        }
        Ok(())
    }
}
