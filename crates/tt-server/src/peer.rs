//! Peering: links between member servers, the registry-driven inventory,
//! and pairing (invite and join).
//!
//! A link is a websocket running the automerge-repo protocol over mutual
//! key-pinned TLS ([`crate::tls`]) on the peer listener, keyed by the server
//! id of the key proven in the handshake. Inbound frames pass a gate that
//! only lets the registry closure through: pushes of other documents are
//! held until the closure lists them (a registry or index edit is usually a
//! moment behind) and close the link after `pending_timeout`.
//!
//! On every link and every change of the closure the server opens every
//! closure document (loading evicted ones, requesting missing ones) and
//! announces it, so members replicate everything, not just what happens to
//! be loaded.
//!
//! Before the automerge-repo protocol, both sides exchange a [`Hello`] with
//! their advertised addresses and hints about other members, kept in the
//! local address store ([`crate::links`]). One dialer task per trusted
//! member keeps it linked, dialing its known addresses with backoff; `--peer`
//! seeds are dialed until the member answering on them is known.
//!
//! Pairing runs on the same listener under ALPN `tt-pair/1`: one JSON line
//! each way, then the joining side fetches the root over a sync link.

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, RwLock, Weak},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use automerge_repo::{
    DocumentId, DocumentStatus, Error as RepoError, PeerId,
    network::NetworkTransport,
    protocol::{PROTOCOL_VERSION, PeerMetadata, WireMessage},
};
use futures_util::{SinkExt, StreamExt};
use rand_core::{OsRng, RngCore};
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Notify, watch},
    time::{Instant, sleep_until, timeout},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use tracing::{debug, info, warn};

use crate::{
    access::AccessIndex,
    access::server_peer,
    admin::AdminError,
    app::{App, RootState},
    auth::{RateLimiter, secret_hash},
    db::{InviteState, JoinIntent, now_ms},
    identity::{Identity, base32, base32_decode, host_name, id_prefix, server_id},
    links::{
        Attempt, Backoff, Hello, Hint, HintAddr, MAX_ADDRESSES, advertised_addresses, dial_loop,
        interface_addresses,
    },
    registry::{self, NewServer, ServerEntry},
    tls::{self, ALPN_PAIR, ALPN_SYNC, PeerAcceptor, PeerTls, Purpose},
    transport::PeerState,
};

/// How long an invite code stays valid.
pub const INVITE_TTL: Duration = Duration::from_secs(600);
const HANDSHAKE: Duration = Duration::from_secs(10);
/// How often held documents are checked against the closure.
const RECHECK: Duration = Duration::from_millis(100);
/// How long an index edit received from a peer keeps its listing re-read.
const DIRTY_TICKS: u8 = 5;
const CODE_PREFIX: &str = "tt-pair:";
const CODE_VERSION: u8 = 1;
/// Pairing attempts allowed per IP per minute.
const PAIR_LIMIT: usize = 5;
/// How often an idle link is pinged.
const KEEPALIVE: Duration = Duration::from_secs(30);
/// How often unconfirmed hints are pruned.
const PRUNE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Per-server peering state: TLS material, dialers, pairing limiter.
pub(crate) struct Peering {
    tls: PeerTls,
    acceptor: PeerAcceptor,
    sync_client: Arc<ClientConfig>,
    /// Wakes the dialer task of each member.
    dialers: Mutex<HashMap<String, Arc<Notify>>>,
    /// Seeds being dialed until their owner is known.
    seeds: Mutex<HashSet<String>>,
    /// The address put into invite codes.
    advertised: RwLock<Option<String>>,
    /// The bound peer listener.
    listen: RwLock<Option<SocketAddr>>,
    limiter: RateLimiter,
    /// Bumped after every completed pairing on the inviting side.
    paired: watch::Sender<u64>,
}

impl Peering {
    pub(crate) fn new(identity: &Identity, access: &Arc<AccessIndex>) -> Result<Self> {
        let tls = PeerTls::new(identity)?;
        let trust: tls::Trust = {
            let access = access.clone();
            Arc::new(move |key: &[u8]| access.is_trusted_key(key))
        };
        Ok(Self {
            acceptor: PeerAcceptor::new(&tls, trust.clone())?,
            sync_client: tls.client(ALPN_SYNC, trust)?,
            tls,
            dialers: Mutex::new(HashMap::new()),
            seeds: Mutex::new(HashSet::new()),
            advertised: RwLock::new(None),
            listen: RwLock::new(None),
            limiter: RateLimiter::new(PAIR_LIMIT, Duration::from_secs(60)),
            paired: watch::Sender::new(0),
        })
    }
}

// ---------- invite codes ----------

/// The content of an invite code: where to reach the inviter, which key it
/// must present, and the one-time secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteCode {
    pub addr: String,
    pub server_id: String,
    pub secret: [u8; 32],
}

impl InviteCode {
    /// `tt-pair:<base32(version, secret, server id, address)>`.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut payload = vec![CODE_VERSION];
        payload.extend_from_slice(&self.secret);
        payload.extend_from_slice(&base32_decode(&self.server_id).unwrap_or_default());
        payload.extend_from_slice(self.addr.as_bytes());
        format!("{CODE_PREFIX}{}", base32(&payload))
    }

    pub fn decode(code: &str) -> Result<Self> {
        let invalid = || AdminError::Invalid("this is not a valid tt-server invite code".into());
        let body = code.trim().strip_prefix(CODE_PREFIX).ok_or_else(invalid)?;
        let payload = base32_decode(body).ok_or_else(invalid)?;
        if payload.len() < 1 + 32 + 16 + 1 || payload[0] != CODE_VERSION {
            return Err(invalid().into());
        }
        let secret: [u8; 32] = payload[1..33].try_into().expect("32 bytes");
        let server_id = base32(&payload[33..49]);
        let addr = String::from_utf8(payload[49..].to_vec()).map_err(|_| invalid())?;
        Ok(Self {
            addr,
            server_id,
            secret,
        })
    }

    fn secret_hash(&self) -> String {
        secret_hash(&registry::hex(&self.secret))
    }
}

/// A fresh invite, as `peer invite` prints it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invitation {
    pub code: String,
    pub addr: String,
    pub expires: i64,
}

/// What `peer join` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinReport {
    /// `joined` (this server adopted the inviter's root), `adopted` (the
    /// inviter adopted this server's root), or `same_root`.
    pub outcome: String,
    /// The inviter, as `name (id prefix)`.
    pub inviter: String,
    pub registry_doc: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PairRequest {
    /// Hex of the invite secret.
    secret: String,
    name: String,
    /// The joiner's registry document, if it has a root.
    root: Option<String>,
    /// Where the inviter can dial the joiner, if it listens.
    addr: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum PairReply {
    /// The joiner adopts `root`; the inviter listed it. The joiner sends a
    /// [`PairDone`] line once it stored the root.
    JoinerAdopts {
        root: String,
        name: String,
    },
    /// The inviter adopts the joiner's root; the joiner lists the inviter
    /// and serves the pull, then a [`PairDone`] line follows.
    InviterAdopts {
        name: String,
    },
    /// Same root: the inviter listed the joiner, the joiner lists the
    /// inviter.
    SameRoot {
        name: String,
    },
    Refused {
        error: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct PairDone {
    ready: bool,
    error: Option<String>,
}

async fn read_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    wait: Duration,
) -> Result<String> {
    let mut line = String::new();
    let read = timeout(wait, reader.read_line(&mut line))
        .await
        .map_err(|_| anyhow!("the other server did not answer in time"))??;
    if read == 0 {
        bail!("the other server closed the connection");
    }
    Ok(line)
}

async fn write_json<W: AsyncWrite + Unpin>(writer: &mut W, value: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

// ---------- links ----------

/// Runs the `join`/`peer` exchange on a fresh link.
async fn handshake<S>(socket: &mut WebSocketStream<S>, local: &PeerId, dialer: bool) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let metadata = PeerMetadata {
        storage_id: None,
        is_ephemeral: Some(false),
    };
    let next = async |socket: &mut WebSocketStream<S>| -> Result<WireMessage> {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => return Ok(WireMessage::decode(&bytes)?),
                Some(Ok(Message::Close(_))) | None => bail!("closed during the handshake"),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
            }
        }
    };
    if dialer {
        let join = WireMessage::Join {
            sender_id: local.to_string(),
            peer_metadata: metadata,
            supported_protocol_versions: vec![PROTOCOL_VERSION.into()],
        };
        socket.send(Message::Binary(join.encode()?.into())).await?;
        match timeout(HANDSHAKE, next(socket)).await?? {
            WireMessage::Peer {
                selected_protocol_version,
                ..
            } if selected_protocol_version == PROTOCOL_VERSION => Ok(()),
            WireMessage::Error { message, .. } => bail!("refused: {message}"),
            other => bail!("unexpected {} during the handshake", other.type_name()),
        }
    } else {
        match timeout(HANDSHAKE, next(socket)).await?? {
            WireMessage::Join {
                sender_id,
                supported_protocol_versions,
                ..
            } if supported_protocol_versions.is_empty()
                || supported_protocol_versions
                    .iter()
                    .any(|version| version == PROTOCOL_VERSION) =>
            {
                let peer = WireMessage::Peer {
                    sender_id: local.to_string(),
                    target_id: sender_id,
                    selected_protocol_version: PROTOCOL_VERSION.into(),
                    peer_metadata: metadata,
                };
                socket.send(Message::Binary(peer.encode()?.into())).await?;
                Ok(())
            }
            other => bail!("unexpected {} during the handshake", other.type_name()),
        }
    }
}

/// Sends this side's hello and reads the other side's.
async fn exchange_hello<S>(socket: &mut WebSocketStream<S>, ours: &Hello) -> Result<Hello>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    socket.send(Message::Binary(ours.encode()?.into())).await?;
    let frame = timeout(HANDSHAKE, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => return Ok(bytes),
                Some(Ok(Message::Close(_))) | None => bail!("closed before the hello"),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
            }
        }
    })
    .await
    .map_err(|_| anyhow!("no hello in time"))??;
    Hello::decode(&frame)
}

/// Why dialing an address did not link.
enum DialError {
    /// Nothing answered: the member is offline (or not there).
    Unreachable(anyhow::Error),
    /// Something answered, but the link failed.
    Failed(anyhow::Error),
}

/// A member's state, as `peer ls` and `/api/peers` report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerStatus {
    pub id: String,
    pub name: String,
    pub state: PeerState,
    /// Documents not yet in sync (0 unless `syncing`).
    pub pending: usize,
    /// UTC seconds of the last link; `None` if never linked.
    pub last_seen: Option<i64>,
    /// The address of the current or last link.
    pub address: Option<String>,
    /// The member's client URL from its latest hello.
    pub public_url: Option<String>,
    pub error: Option<String>,
}

struct Held {
    deadline: Instant,
    frames: Vec<Vec<u8>>,
}

/// Admits the registry closure from a server peer.
struct PeerGate {
    app: Arc<App>,
    server: String,
    pending: HashMap<DocumentId, Held>,
    /// Users whose index a peer just changed: their listing is re-read on
    /// the next few ticks, once the repository applied the change.
    dirty: HashMap<String, u8>,
}

impl PeerGate {
    /// Frames to deliver now, or the reason to close the link.
    fn inbound(&mut self, frame: Vec<u8>) -> Result<Vec<Vec<u8>>, String> {
        let Ok(message) = WireMessage::decode(&frame) else {
            // Undecodable frames are the repository's to reject.
            return Ok(vec![frame]);
        };
        let WireMessage::Sync { document_id, .. } = message else {
            // A request outside the closure is answered `doc-unavailable`.
            return Ok(vec![frame]);
        };
        if let Some(held) = self.pending.get_mut(&document_id) {
            held.frames.push(frame);
            return Ok(Vec::new());
        }
        if self.app.access.in_closure(document_id) {
            if let Some(user) = self.app.access.index_owner(document_id) {
                self.dirty.insert(user, DIRTY_TICKS);
            }
            return Ok(vec![frame]);
        }
        self.pending.insert(
            document_id,
            Held {
                deadline: Instant::now() + self.app.options.pending_timeout,
                frames: vec![frame],
            },
        );
        Ok(Vec::new())
    }

    /// Re-reads changed listings and releases held documents the closure
    /// now contains.
    async fn tick(&mut self) -> Vec<Vec<u8>> {
        let mut users: Vec<String> = self.dirty.keys().cloned().collect();
        if !self.pending.is_empty() {
            // Something waits for a listing: re-read them all.
            users = self
                .app
                .access
                .members()
                .into_iter()
                .map(|member| member.id)
                .collect();
        }
        for user in users {
            if let Err(error) = self.app.refresh_listing(&user).await {
                warn!(error = %format!("{error:#}"), "re-reading an index failed");
            }
        }
        self.dirty.retain(|_, ticks| {
            *ticks -= 1;
            *ticks > 0
        });
        let ready: Vec<DocumentId> = self
            .pending
            .keys()
            .filter(|id| self.app.access.in_closure(**id))
            .copied()
            .collect();
        let mut frames = Vec::new();
        for id in ready {
            debug!(server = %self.server, document = %id.to_bs58check(), "accepted a document from a peer");
            if let Some(held) = self.pending.remove(&id) {
                frames.extend(held.frames);
            }
        }
        frames
    }

    fn expire(&self) -> Result<(), String> {
        let now = Instant::now();
        match self.pending.iter().find(|(_, held)| held.deadline <= now) {
            Some((id, _)) => Err(format!(
                "document {} is not reachable from the registry",
                id.to_bs58check()
            )),
            None => Ok(()),
        }
    }

    fn busy(&self) -> bool {
        !self.pending.is_empty() || !self.dirty.is_empty()
    }
}

async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

impl App {
    /// Records the bound peer listener; invite codes carry the first
    /// `--peer-advertise` value, or the listen address (the host name when
    /// it binds every interface).
    pub fn set_peer_listener(&self, listen: SocketAddr) {
        let invite = self
            .options
            .peer_advertise
            .first()
            .cloned()
            .unwrap_or_else(|| {
                if listen.ip().is_unspecified() {
                    format!("{}:{}", host_name(), listen.port())
                } else {
                    listen.to_string()
                }
            });
        *self.peering.listen.write().unwrap() = Some(listen);
        *self.peering.advertised.write().unwrap() = Some(invite);
    }

    /// The addresses this server advertises in its hello, recomputed from
    /// the interfaces on every call. Nothing without a peer listener.
    #[must_use]
    pub fn advertised(&self) -> Vec<String> {
        match *self.peering.listen.read().unwrap() {
            Some(listen) => {
                advertised_addresses(listen, &self.options.peer_advertise, &interface_addresses())
            }
            None => Vec::new(),
        }
    }

    #[must_use]
    pub fn peer_address(&self) -> Option<String> {
        self.peering.advertised.read().unwrap().clone()
    }

    /// Server ids with an open link.
    #[must_use]
    pub fn linked_servers(&self) -> Vec<String> {
        self.transport.linked()
    }

    /// Watches the set of linked server ids.
    #[must_use]
    pub fn subscribe_links(&self) -> watch::Receiver<Vec<String>> {
        self.transport.subscribe_links()
    }

    /// Runs one link until it ends: hellos, handshake, then frames both
    /// ways through the gate. `address` is the dialed address, or the
    /// remote address of an inbound link. `false` when the link was refused
    /// as a duplicate.
    async fn run_link<S>(
        self: &Arc<Self>,
        mut socket: WebSocketStream<S>,
        server: String,
        dialer: bool,
        address: Option<String>,
    ) -> Result<bool>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        if server == self.identity().server_id() {
            bail!("refusing a link to this server itself");
        }
        let ours = self.own_hello(&server).await;
        let theirs = exchange_hello(&mut socket, &ours).await?;
        if theirs.server_id != server {
            bail!(
                "the hello names server {} but the key is that of {}",
                id_prefix(&theirs.server_id),
                id_prefix(&server)
            );
        }
        if let Err(error) = self.accept_hello(&server, &theirs).await {
            warn!(error = %format!("{error:#}"), "storing peer addresses failed");
        }
        handshake(&mut socket, &self.transport.local_peer(), dialer).await?;
        // Both sides keep the link dialed by the lower server id.
        let preferred = dialer == (self.identity().server_id() < server.as_str());
        let Some(mut attached) = self
            .transport
            .attach(&server, preferred, address.clone())
            .await
        else {
            debug!(server = %self.server_display(&server), "duplicate link; keeping the one dialed by the lower server id");
            let _ = socket.close(None).await;
            return Ok(false);
        };
        info!(server = %self.server_display(&server), dialer, address = address.as_deref().unwrap_or("?"), "peer linked");
        {
            let (id, addr, url, now) = (
                server.clone(),
                address.clone(),
                theirs.public_url.clone(),
                now_ms(),
            );
            let recorded = self
                .db
                .run(move |db| {
                    db.record_link(&id, addr.as_deref(), url.as_deref(), now)?;
                    if let Some(addr) = addr.filter(|_| dialer) {
                        db.confirm_address(&id, &addr, now)?;
                    }
                    Ok(())
                })
                .await;
            if let Err(error) = recorded {
                warn!(error = %format!("{error:#}"), "recording a peer link failed");
            }
        }
        let mut gate = PeerGate {
            app: self.clone(),
            server: server.clone(),
            pending: HashMap::new(),
            dirty: HashMap::new(),
        };
        let mut recheck = tokio::time::interval(RECHECK);
        let mut keepalive = tokio::time::interval(KEEPALIVE);
        keepalive.reset();
        let reason = loop {
            let deadline = gate.pending.values().map(|held| held.deadline).min();
            tokio::select! {
                frame = socket.next() => match frame {
                    Some(Ok(Message::Binary(bytes))) => match gate.inbound(bytes.to_vec()) {
                        Ok(frames) => for frame in frames {
                            self.transport.deliver(&attached.peer, frame).await;
                        },
                        Err(reason) => break Some(reason),
                    },
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break None,
                    Some(Ok(_)) => {}
                },
                outgoing = attached.outgoing.recv() => match outgoing {
                    Some(frame) => {
                        if socket.send(Message::Binary(frame.into())).await.is_err() {
                            break None;
                        }
                    }
                    None => break None,
                },
                () = attached.close.notified() => break None,
                () = attached.probe.notified() => {
                    if socket.send(Message::Ping(Default::default())).await.is_err() {
                        break None;
                    }
                }
                _ = keepalive.tick() => {
                    if socket.send(Message::Ping(Default::default())).await.is_err() {
                        break None;
                    }
                }
                _ = recheck.tick(), if gate.busy() => {
                    for frame in gate.tick().await {
                        self.transport.deliver(&attached.peer, frame).await;
                    }
                }
                () = until(deadline) => if let Err(reason) = gate.expire() {
                    break Some(reason);
                },
            }
        };
        if let Some(reason) = &reason {
            warn!(server = %self.server_display(&server), %reason, "closing a peer link");
            let error = WireMessage::Error {
                sender_id: self.transport.local_peer().to_string(),
                target_id: None,
                message: reason.clone(),
            };
            if let Ok(frame) = error.encode() {
                let _ = socket.send(Message::Binary(frame.into())).await;
            }
        }
        let _ = socket.close(None).await;
        self.transport.detach(&server, attached.generation).await;
        if let Some(reason) = reason {
            self.transport.record_error(&server, Some(reason));
        }
        let (id, now) = (server.clone(), now_ms());
        let _ = self.db.run(move |db| db.touch_link(&id, now)).await;
        info!(server = %self.server_display(&server), "peer link closed");
        Ok(true)
    }

    /// This server's hello to `to`: advertised addresses, the client URL,
    /// and the known addresses of every other member.
    async fn own_hello(&self, to: &str) -> Hello {
        let mut hello = Hello::new(self.identity().server_id(), &self.own_name());
        hello.advertise = self.advertised();
        hello.public_url = self.options.public_url.clone();
        let own = self.identity().server_id().to_owned();
        let members: Vec<String> = self
            .view()
            .servers()
            .into_iter()
            .filter(|entry| !entry.is_revoked() && entry.id != own && entry.id != to)
            .map(|entry| entry.id.clone())
            .collect();
        let known = self
            .db
            .run(move |db| {
                members
                    .into_iter()
                    .map(|id| {
                        let addresses = db.member_addresses(&id)?;
                        Ok((id, addresses))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .await;
        match known {
            Ok(known) => {
                for (id, addresses) in known {
                    if addresses.is_empty() {
                        continue;
                    }
                    hello.hints.push(Hint {
                        server_id: id,
                        addrs: addresses
                            .into_iter()
                            .take(MAX_ADDRESSES)
                            .map(|address| HintAddr {
                                seen_at: address.last_ok.unwrap_or(address.last_seen) / 1000,
                                addr: address.addr,
                            })
                            .collect(),
                    });
                }
            }
            Err(error) => warn!(error = %format!("{error:#}"), "reading peer addresses failed"),
        }
        hello
    }

    /// Whether the registry lists `id` and does not revoke it.
    fn is_member(&self, id: &str) -> bool {
        self.view()
            .server(id)
            .is_some_and(|entry| !entry.is_revoked())
    }

    /// Stores what a member's hello carries: its advertised addresses (also
    /// those of the server a join fetches from, which the registry lists
    /// once it arrives), and hints for other members this registry lists
    /// and does not revoke (at most [`MAX_ADDRESSES`] each). Everything else
    /// is discarded.
    async fn accept_hello(&self, server: &str, hello: &Hello) -> Result<()> {
        let now = now_ms();
        let own = self.identity().server_id().to_owned();
        if self.access.is_trusted_server(server) {
            let (id, advertised): (String, Vec<String>) = (
                server.to_owned(),
                hello
                    .advertise
                    .iter()
                    .take(MAX_ADDRESSES)
                    .cloned()
                    .collect(),
            );
            self.db
                .run(move |db| db.set_advertised(&id, &advertised, now))
                .await?;
        }
        for hint in &hello.hints {
            if hint.server_id == own || hint.server_id == server {
                continue;
            }
            if !self.is_member(&hint.server_id) {
                debug!(member = %id_prefix(&hint.server_id), "discarding addresses of a server that is not a member");
                continue;
            }
            let (id, addrs): (String, Vec<String>) = (
                hint.server_id.clone(),
                hint.addrs
                    .iter()
                    .take(MAX_ADDRESSES)
                    .map(|addr| addr.addr.clone())
                    .collect(),
            );
            self.db
                .run(move |db| db.add_hints(&id, &addrs, now))
                .await?;
            self.wake_dialer(&hint.server_id);
        }
        Ok(())
    }

    fn server_display(&self, id: &str) -> String {
        self.view()
            .server(id)
            .map_or_else(|| id_prefix(id).to_owned(), ServerEntry::display)
    }

    /// Accepts peer connections on `listener` until the task is stopped.
    pub(crate) async fn serve_peers(self: Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, from)) => {
                    let app = self.clone();
                    tokio::spawn(async move {
                        if let Err(error) = app.inbound(stream, from).await {
                            debug!(%from, error = %format!("{error:#}"), "peer connection ended");
                        }
                    });
                }
                Err(error) => {
                    warn!(%error, "peer listener accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn inbound(self: Arc<Self>, stream: TcpStream, from: SocketAddr) -> Result<()> {
        let (stream, purpose, key) = timeout(HANDSHAKE, self.peering.acceptor.accept(stream))
            .await
            .map_err(|_| anyhow!("TLS handshake timed out"))??;
        match purpose {
            Purpose::Sync => {
                let socket = timeout(HANDSHAKE, tokio_tungstenite::accept_async(stream)).await??;
                self.run_link(socket, server_id(&key), false, Some(from.ip().to_string()))
                    .await
                    .map(|_| ())
            }
            Purpose::Pair => self.pair_inbound(stream, key, from.ip()).await,
        }
    }

    /// Dials `addr` and runs the link until it ends; `Ok(false)` when it
    /// was refused as a duplicate. `expected` is the member the address
    /// belongs to; for a seed (`None`) the member answering is recorded.
    async fn dial_addr(
        self: &Arc<Self>,
        addr: &str,
        expected: Option<&str>,
    ) -> Result<bool, DialError> {
        let tcp = timeout(HANDSHAKE, TcpStream::connect(addr))
            .await
            .map_err(|_| DialError::Unreachable(anyhow!("connecting to {addr} timed out")))?
            .map_err(|error| {
                DialError::Unreachable(anyhow!(error).context(format!("connecting to {addr}")))
            })?;
        let failed = DialError::Failed;
        let (stream, key) = timeout(
            HANDSHAKE,
            tls::connect(tcp, self.peering.sync_client.clone()),
        )
        .await
        .map_err(|_| failed(anyhow!("TLS handshake with {addr} timed out")))?
        .map_err(|error| failed(error.context(format!("TLS handshake with {addr}"))))?;
        let server = server_id(&key);
        match expected {
            Some(expected) if expected != server => {
                return Err(failed(anyhow!(
                    "{addr} answered as server {}, not {}",
                    id_prefix(&server),
                    id_prefix(expected)
                )));
            }
            Some(_) => {}
            None => {
                let (seed, id) = (addr.to_owned(), server.clone());
                self.db
                    .run(move |db| db.resolve_seed(&seed, &id))
                    .await
                    .map_err(failed)?;
                info!(%addr, server = %self.server_display(&server), "seed answered");
            }
        }
        let (socket, _) = timeout(
            HANDSHAKE,
            tokio_tungstenite::client_async("ws://tt-peer/", stream),
        )
        .await
        .map_err(|_| failed(anyhow!("websocket upgrade with {addr} timed out")))?
        .map_err(|error| failed(error.into()))?;
        self.run_link(socket, server, true, Some(addr.to_owned()))
            .await
            .map_err(failed)
    }

    /// One pass over a member's known addresses, in dial order.
    async fn dial_member(self: &Arc<Self>, member: &str) -> Attempt {
        let id = member.to_owned();
        let addresses = match self.db.run(move |db| db.dial_order(&id)).await {
            Ok(addresses) => addresses,
            Err(error) => {
                warn!(error = %format!("{error:#}"), "reading peer addresses failed");
                return Attempt::Failed;
            }
        };
        if addresses.is_empty() {
            return Attempt::NoAddress;
        }
        let mut last_error = None;
        for addr in addresses {
            match self.dial_addr(&addr, Some(member)).await {
                Ok(_) => return Attempt::Linked,
                Err(DialError::Unreachable(error)) => {
                    debug!(%addr, error = %format!("{error:#}"), "member unreachable");
                }
                Err(DialError::Failed(error)) => {
                    debug!(%addr, error = %format!("{error:#}"), "dialing a member failed");
                    last_error = Some(format!("{error:#}"));
                }
            }
            if self.transport.linked().iter().any(|id| id == member) {
                // It dialed this server meanwhile.
                return Attempt::Linked;
            }
        }
        self.transport.record_error(member, last_error);
        Attempt::Failed
    }

    /// Starts the dialer task of `member` unless it runs.
    pub(crate) fn ensure_dialer(self: &Arc<Self>, member: &str) {
        let wake = {
            let mut dialers = self.peering.dialers.lock().unwrap();
            if dialers.contains_key(member) {
                return;
            }
            let wake = Arc::new(Notify::new());
            dialers.insert(member.to_owned(), wake.clone());
            wake
        };
        let app = Arc::downgrade(self);
        let links = self.transport.subscribe_links();
        let backoff = Backoff::new(self.options.peer_retry, self.options.peer_retry_max);
        let id = member.to_owned();
        let task = tokio::spawn(async move {
            let keep = {
                let (app, id) = (app.clone(), id.clone());
                move || {
                    app.upgrade()
                        .is_some_and(|app| app.access.is_trusted_server(&id))
                }
            };
            let attempt = {
                let (app, id) = (app.clone(), id.clone());
                move || {
                    let (app, id) = (app.clone(), id.clone());
                    async move {
                        match app.upgrade() {
                            Some(app) => app.dial_member(&id).await,
                            None => Attempt::NoAddress,
                        }
                    }
                }
            };
            dial_loop(id.clone(), links, wake, backoff, keep, attempt).await;
            if let Some(app) = app.upgrade() {
                app.peering.dialers.lock().unwrap().remove(&id);
                debug!(member = %id_prefix(&id), "stopped dialing");
            }
        });
        self.tasks.lock().unwrap().push(task);
    }

    /// Cuts the current wait of `member`'s dialer short.
    fn wake_dialer(&self, member: &str) {
        if let Some(wake) = self.peering.dialers.lock().unwrap().get(member) {
            wake.notify_one();
        }
    }

    /// Keeps one dialer per trusted server (members and a join source);
    /// dialers of servers no longer trusted are woken to stop.
    fn watch_dialers(self: &Arc<Self>) {
        let mut trusted = self.access.subscribe_trusted();
        let app = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let ids = trusted.borrow_and_update().clone();
                {
                    let Some(app) = app.upgrade() else { return };
                    for id in &ids {
                        app.ensure_dialer(id);
                    }
                    let stale: Vec<Arc<Notify>> = app
                        .peering
                        .dialers
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(id, _)| !ids.contains(id))
                        .map(|(_, wake)| wake.clone())
                        .collect();
                    for wake in stale {
                        wake.notify_one();
                    }
                }
                if trusted.changed().await.is_err() {
                    return;
                }
            }
        });
        self.tasks.lock().unwrap().push(task);
    }

    /// Records a `--peer` seed and dials it until the member answering on
    /// it is known; from then on that member's dialer uses it.
    pub fn add_seed(self: &Arc<Self>, addr: String) {
        if !self.peering.seeds.lock().unwrap().insert(addr.clone()) {
            return;
        }
        let app = Arc::downgrade(self);
        let mut backoff = Backoff::new(self.options.peer_retry, self.options.peer_retry_max);
        let task = tokio::spawn(async move {
            {
                let Some(strong) = app.upgrade() else { return };
                let seed = addr.clone();
                if let Err(error) = strong.db.run(move |db| db.add_seed(&seed)).await {
                    warn!(%addr, error = %format!("{error:#}"), "recording a seed failed");
                }
            }
            loop {
                let Some(strong) = app.upgrade() else { return };
                let unresolved = strong.db.run(|db| db.unresolved_seeds()).await;
                if unresolved.is_ok_and(|seeds| !seeds.contains(&addr)) {
                    strong.peering.seeds.lock().unwrap().remove(&addr);
                    return;
                }
                if let Err(DialError::Unreachable(error) | DialError::Failed(error)) =
                    strong.dial_addr(&addr, None).await
                {
                    debug!(%addr, error = %format!("{error:#}"), "dialing a seed failed");
                }
                drop(strong);
                tokio::time::sleep(backoff.next_delay()).await;
            }
        });
        self.tasks.lock().unwrap().push(task);
    }

    /// Starts peering for a serving server: prunes stale hints (now and
    /// daily), dials unresolved seeds, and keeps a dialer per member.
    pub(crate) async fn start_peering(self: &Arc<Self>) -> Result<()> {
        let pruned = self.db.run(|db| db.prune_hints(now_ms())).await?;
        if pruned > 0 {
            info!(pruned, "pruned peer addresses unconfirmed for 30 days");
        }
        for seed in self.db.run(|db| db.unresolved_seeds()).await? {
            self.add_seed(seed);
        }
        self.watch_dialers();
        let app = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let mut daily = tokio::time::interval(PRUNE_EVERY);
            daily.reset();
            loop {
                daily.tick().await;
                let Some(app) = app.upgrade() else { return };
                if let Err(error) = app.db.run(|db| db.prune_hints(now_ms())).await {
                    warn!(error = %format!("{error:#}"), "pruning peer addresses failed");
                }
            }
        });
        self.tasks.lock().unwrap().push(task);
        Ok(())
    }

    /// Every non-revoked member except this server, with its link state.
    pub async fn peer_status(&self) -> Result<Vec<PeerStatus>> {
        let seen = self.db.run(|db| db.peer_seen()).await?;
        let health = self.transport.health();
        let progress = self.repo.peer_sync_progress();
        let own = self.identity().server_id().to_owned();
        let now = now_ms();
        let mut peers: Vec<PeerStatus> = self
            .view()
            .servers()
            .into_iter()
            .filter(|entry| !entry.is_revoked() && entry.id != own)
            .map(|entry| {
                let link = health.get(&entry.id).cloned().unwrap_or_default();
                let stored = seen.get(&entry.id);
                let pending = if link.linked {
                    progress
                        .get(&server_peer(&entry.id))
                        .map_or(0, |progress| progress.syncing_documents.len())
                } else {
                    0
                };
                let state = link.state(pending);
                let last_seen = if link.linked {
                    Some(now)
                } else {
                    link.last_seen
                        .into_iter()
                        .chain(stored.map(|stored| stored.last_seen))
                        .max()
                };
                PeerStatus {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    state,
                    pending,
                    last_seen: last_seen.map(|ms| ms.div_euclid(1000)),
                    address: link
                        .address
                        .or_else(|| stored.and_then(|stored| stored.address.clone())),
                    public_url: stored.and_then(|stored| stored.public_url.clone()),
                    error: link.error.filter(|_| state == PeerState::Error),
                }
            })
            .collect();
        peers.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        Ok(peers)
    }

    // ---------- inventory ----------

    /// Opens and announces every closure document; requests the missing
    /// ones from linked members.
    pub(crate) async fn inventory(&self) {
        if self.transport.linked().is_empty() {
            return;
        }
        for id in self.access.closure() {
            match self.repo.open_document(id).await {
                Ok(handle) => {
                    drop(handle);
                    let _ = self.repo.announce(id).await;
                }
                Err(RepoError::NotFound(_)) => {
                    let _ = self.repo.find(id).await;
                }
                Err(error) => {
                    warn!(document = %id.to_bs58check(), %error, "opening a document for peers failed")
                }
            }
        }
    }

    /// Runs the inventory whenever the closure or the set of links changes.
    pub(crate) fn watch_inventory(self: &Arc<Self>) {
        let mut closure = self.access.subscribe_closure();
        let mut links = self.transport.subscribe_links();
        let app = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = closure.changed() => if changed.is_err() { return },
                    changed = links.changed() => if changed.is_err() { return },
                }
                // Coalesce bursts (a registry change and its index reads).
                tokio::time::sleep(Duration::from_millis(50)).await;
                closure.mark_unchanged();
                links.mark_unchanged();
                let Some(app) = app.upgrade() else { return };
                app.inventory().await;
            }
        });
        self.tasks.lock().unwrap().push(task);
    }

    // ---------- membership ----------

    /// Every member server, revoked ones included.
    pub fn servers(&self) -> Result<Vec<ServerEntry>> {
        self.registry()?;
        Ok(self.view().servers().into_iter().cloned().collect())
    }

    /// A member by exact name, or else by id prefix. Ambiguous names need an
    /// id prefix.
    fn resolve_server(&self, target: &str) -> Result<ServerEntry> {
        let view = self.view();
        let named: Vec<&ServerEntry> = view
            .servers()
            .into_iter()
            .filter(|entry| entry.name == target)
            .collect();
        match named.as_slice() {
            [one] => return Ok((*one).clone()),
            [] => {}
            many => {
                let ids: Vec<String> = many.iter().map(|entry| entry.display()).collect();
                return Err(AdminError::Invalid(format!(
                    "{} members are named {target:?}: {}; give an id prefix instead",
                    many.len(),
                    ids.join(", ")
                ))
                .into());
            }
        }
        let target = target.to_ascii_lowercase();
        let matches: Vec<&ServerEntry> = view
            .servers()
            .into_iter()
            .filter(|entry| entry.id.starts_with(&target))
            .collect();
        match matches.as_slice() {
            [one] if !target.is_empty() => Ok((*one).clone()),
            [] | [_] => Err(AdminError::NoSuchServer(target).into()),
            _ => Err(AdminError::Invalid(format!(
                "{target:?} matches several members; give more of the id"
            ))
            .into()),
        }
    }

    /// Adds `revoked[id]`; the link to it closes at once.
    pub async fn revoke_server(&self, target: &str) -> Result<ServerEntry> {
        let _writer = self.writer.lock().await;
        let entry = self.resolve_server(target)?;
        if entry.id == self.identity().server_id() {
            return Err(AdminError::Invalid(
                "refusing to revoke this server itself; to leave the membership, run `tt-server reset` here and revoke it from another member".into(),
            )
            .into());
        }
        if !entry.is_revoked() {
            let (id, by) = (entry.id.clone(), self.identity().server_id().to_owned());
            self.write_registry(move |tx| registry::revoke_server(tx, &id, &by, now_ms()))
                .await?;
            info!(server = %entry.display(), "member revoked");
        }
        Ok(self.view().server(&entry.id).cloned().unwrap_or(entry))
    }

    pub async fn rename_server(&self, target: &str, new_name: &str) -> Result<ServerEntry> {
        crate::app::check_server_name(new_name)?;
        let _writer = self.writer.lock().await;
        let entry = self.resolve_server(target)?;
        let at = now_ms().max(entry.name_changed_at + 1);
        let (id, name) = (entry.id.clone(), new_name.to_owned());
        self.write_registry(move |tx| registry::set_server_name(tx, &id, &name, at))
            .await?;
        Ok(self.view().server(&entry.id).cloned().unwrap_or(entry))
    }

    /// Lists (or refreshes) a member with the key it proved.
    async fn add_member(&self, pubkey: &[u8], name: &str) -> Result<()> {
        let _writer = self.writer.lock().await;
        let server = NewServer {
            id: server_id(pubkey),
            name: name.to_owned(),
            pubkey: pubkey.to_vec(),
            added_by: self.identity().server_id().to_owned(),
            added_at: now_ms(),
        };
        self.write_registry(move |tx| registry::add_server(tx, &server))
            .await
    }

    /// Lists this server in its own registry when it is missing (roots
    /// created before membership existed).
    pub(crate) async fn register_self(&self) -> Result<()> {
        if self.state() != RootState::Ready
            || self.view().server(self.identity().server_id()).is_some()
        {
            return Ok(());
        }
        let name = self.root().map(|root| root.name).unwrap_or_else(host_name);
        info!(%name, "listing this server in the registry");
        let key = self.identity().public_key().to_vec();
        self.add_member(&key, &name).await
    }

    // ---------- invite ----------

    /// Creates a one-time invite code for the peer listener at `addr` (or
    /// the advertised peer address).
    pub async fn invite(&self, addr: Option<String>, name: Option<String>) -> Result<Invitation> {
        let addr = addr.or_else(|| self.peer_address()).ok_or_else(|| {
            AdminError::Invalid(
                "this server has no peer listener: start `tt-server serve` with --peer-listen, or pass --addr".into(),
            )
        })?;
        if self.state() == RootState::Joining {
            return Err(AdminError::Joining.into());
        }
        let mut secret = [0_u8; 32];
        OsRng.fill_bytes(&mut secret);
        let code = InviteCode {
            addr: addr.clone(),
            server_id: self.identity().server_id().to_owned(),
            secret,
        };
        let expires = now_ms() + i64::try_from(INVITE_TTL.as_millis()).unwrap_or(i64::MAX);
        let hash = code.secret_hash();
        self.db
            .run(move |db| db.insert_invite(&hash, expires, name.as_deref()))
            .await?;
        Ok(Invitation {
            code: code.encode(),
            addr,
            expires,
        })
    }

    /// Waits until the next pairing this server accepts as inviter is
    /// complete; `false` on timeout.
    pub async fn wait_paired(&self, wait: Duration) -> bool {
        let mut paired = self.peering.paired.subscribe();
        paired.mark_unchanged();
        timeout(wait, paired.changed()).await.is_ok()
    }

    /// The inviting side of `tt-pair/1`.
    async fn pair_inbound<S>(
        self: &Arc<Self>,
        stream: S,
        joiner_key: Vec<u8>,
        ip: IpAddr,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = BufReader::new(read);
        let line = read_line(&mut reader, HANDSHAKE).await?;
        let request: PairRequest = serde_json::from_str(&line).context("pairing request")?;
        let reply = match self.accept_pairing(ip, &joiner_key, &request).await {
            Ok(reply) => reply,
            Err(error) => PairReply::Refused {
                error: match error.downcast_ref::<AdminError>() {
                    Some(admin) => admin.to_string(),
                    None => {
                        warn!(error = %format!("{error:#}"), "pairing failed");
                        "internal error".into()
                    }
                },
            },
        };
        write_json(&mut write, &reply).await?;
        match reply {
            PairReply::Refused { error } => {
                warn!(%ip, %error, "pairing refused");
                Ok(())
            }
            PairReply::InviterAdopts { .. } => {
                let done = match self.wait_ready(self.options.join_timeout).await {
                    Ok(()) => PairDone {
                        ready: true,
                        error: None,
                    },
                    Err(error) => PairDone {
                        ready: false,
                        error: Some(format!("{error:#}")),
                    },
                };
                let _ = write_json(&mut write, &done).await;
                self.peering.paired.send_modify(|count| *count += 1);
                Ok(())
            }
            PairReply::JoinerAdopts { .. } => {
                // Keep serving the joiner's pull until it reports.
                match read_line(&mut reader, self.options.join_timeout).await {
                    Ok(line) => match serde_json::from_str::<PairDone>(&line) {
                        Ok(PairDone { ready: true, .. }) => {
                            info!(server = %self.server_display(&server_id(&joiner_key)), "the new member stored the root");
                        }
                        Ok(PairDone { error, .. }) => {
                            warn!(error = %error.unwrap_or_default(), "the new member could not fetch the root");
                        }
                        Err(error) => warn!(%error, "invalid pairing completion"),
                    },
                    Err(error) => {
                        warn!(error = %format!("{error:#}"), "the new member did not report")
                    }
                }
                self.peering.paired.send_modify(|count| *count += 1);
                Ok(())
            }
            PairReply::SameRoot { .. } => {
                self.peering.paired.send_modify(|count| *count += 1);
                Ok(())
            }
        }
    }

    /// Checks the secret and the roots, consumes the secret, and acts.
    async fn accept_pairing(
        self: &Arc<Self>,
        ip: IpAddr,
        joiner_key: &[u8],
        request: &PairRequest,
    ) -> Result<PairReply> {
        if !self.peering.limiter.attempt(ip) {
            return Err(
                AdminError::Invalid("too many pairing attempts; wait a minute".into()).into(),
            );
        }
        let hash = secret_hash(&request.secret);
        let check = hash.clone();
        let invite = self.db.run(move |db| db.invite(&check)).await?;
        match invite.as_ref().map(|invite| invite.state) {
            Some(InviteState::Valid) => {}
            Some(InviteState::Expired) => {
                return Err(AdminError::Invalid(
                    "the invite code has expired (codes are valid for 10 minutes); run `tt-server peer invite` again".into(),
                )
                .into());
            }
            _ => {
                return Err(AdminError::Invalid(
                    "the invite code is invalid or was already used".into(),
                )
                .into());
            }
        }
        let joiner = server_id(joiner_key);
        if joiner == self.identity().server_id() {
            return Err(AdminError::Invalid("a server cannot pair with itself".into()).into());
        }
        let ours = match self.state() {
            RootState::Joining => return Err(AdminError::Joining.into()),
            RootState::Ready => self.root().map(|root| root.registry_doc),
            RootState::NeedsDecision => None,
        };
        let theirs = request.root.clone();
        if let (Some(ours), Some(theirs)) = (&ours, &theirs)
            && ours != theirs
        {
            return Err(AdminError::Invalid(DIFFERENT_ROOTS.into()).into());
        }
        if ours.is_none() && theirs.is_none() {
            return Err(AdminError::Invalid(BOTH_EMPTY.into()).into());
        }
        if ours.is_some()
            && self
                .view()
                .server(&joiner)
                .is_some_and(ServerEntry::is_revoked)
        {
            return Err(AdminError::Invalid(
                "this server was revoked from the membership; run `tt-server reset --new-identity` on it before joining again".into(),
            )
            .into());
        }
        // Consume last: a refused pairing leaves the code usable.
        let consumed = hash.clone();
        match self.db.run(move |db| db.consume_invite(&consumed)).await? {
            InviteState::Valid => {}
            _ => {
                return Err(AdminError::Invalid(
                    "the invite code is invalid or was already used".into(),
                )
                .into());
            }
        }
        let invite_name = invite.and_then(|invite| invite.name);
        match (ours, theirs) {
            (Some(root), None) => {
                self.add_member(joiner_key, &request.name).await?;
                info!(server = %self.server_display(&joiner), "member added; it is fetching the root");
                Ok(PairReply::JoinerAdopts {
                    root,
                    name: self.own_name(),
                })
            }
            (Some(_), Some(_)) => {
                self.add_member(joiner_key, &request.name).await?;
                info!(server = %self.server_display(&joiner), "member listed (same root)");
                if let Some(addr) = &request.addr {
                    self.remember_address(addr, &joiner).await?;
                }
                Ok(PairReply::SameRoot {
                    name: self.own_name(),
                })
            }
            (None, Some(root)) => {
                let name = invite_name.unwrap_or_else(host_name);
                self.begin_joining(JoinIntent {
                    registry_doc: root,
                    source_id: joiner.clone(),
                    source_addr: request.addr.clone(),
                    name: name.clone(),
                    started: now_ms(),
                })
                .await?;
                info!(%joiner, "adopting the joining server's root");
                Ok(PairReply::InviterAdopts { name })
            }
            (None, None) => unreachable!("refused above"),
        }
    }

    fn own_name(&self) -> String {
        self.root().map_or_else(host_name, |root| root.name)
    }

    /// Keeps an address from pairing as a hint for `server` and dials it.
    async fn remember_address(self: &Arc<Self>, addr: &str, server: &str) -> Result<()> {
        let (addrs, id) = (vec![addr.to_owned()], server.to_owned());
        self.db
            .run(move |db| db.add_hints(&id, &addrs, now_ms()))
            .await?;
        self.ensure_dialer(server);
        self.wake_dialer(server);
        Ok(())
    }

    // ---------- join ----------

    /// The joining side of `tt-pair/1`: verifies the inviter's key before
    /// sending the secret, then adopts, serves, or refreshes as the roots
    /// decide. Returns once this server is `Ready` (or, when the inviter
    /// adopts this root, once the inviter reports it is).
    pub async fn join(self: &Arc<Self>, code: &str, name: Option<String>) -> Result<JoinReport> {
        let invite = InviteCode::decode(code)?;
        if invite.server_id == self.identity().server_id() {
            return Err(
                AdminError::Invalid("this invite code was made by this server".into()).into(),
            );
        }
        let root = match self.state() {
            RootState::Joining => return Err(AdminError::Joining.into()),
            RootState::Ready => self.root().map(|root| root.registry_doc),
            RootState::NeedsDecision => None,
        };
        let expected = invite.server_id.clone();
        let pin: tls::Trust = Arc::new(move |key: &[u8]| server_id(key) == expected);
        let config = self.peering.tls.client(ALPN_PAIR, pin)?;
        let tcp = timeout(HANDSHAKE, TcpStream::connect(&invite.addr))
            .await
            .map_err(|_| anyhow!("connecting to {} timed out", invite.addr))?
            .with_context(|| format!("connecting to {}", invite.addr))?;
        let (stream, inviter_key) = timeout(HANDSHAKE, tls::connect(tcp, config))
            .await
            .map_err(|_| anyhow!("TLS handshake with {} timed out", invite.addr))?
            .map_err(|error| {
                AdminError::Invalid(format!(
                    "the server at {} did not prove the key of server {} from the invite code ({error:#}); nothing was sent",
                    invite.addr,
                    id_prefix(&invite.server_id)
                ))
            })?;
        let own_name = name.unwrap_or_else(|| self.own_name());
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = BufReader::new(read);
        let request = PairRequest {
            secret: registry::hex(&invite.secret),
            name: own_name.clone(),
            root: root.clone(),
            addr: self.peer_address(),
        };
        write_json(&mut write, &request).await?;
        let reply: PairReply = serde_json::from_str(&read_line(&mut reader, HANDSHAKE * 3).await?)
            .context("pairing reply")?;
        let inviter = invite.server_id.clone();
        match reply {
            PairReply::Refused { error } => Err(AdminError::Invalid(error).into()),
            PairReply::JoinerAdopts { root, name } => {
                self.begin_joining(JoinIntent {
                    registry_doc: root.clone(),
                    source_id: inviter.clone(),
                    source_addr: Some(invite.addr.clone()),
                    name: own_name,
                    started: now_ms(),
                })
                .await?;
                self.remember_address(&invite.addr, &inviter).await?;
                let ready = self.wait_ready(self.options.join_timeout).await;
                let done = PairDone {
                    ready: ready.is_ok(),
                    error: ready.as_ref().err().map(|error| format!("{error:#}")),
                };
                let _ = write_json(&mut write, &done).await;
                ready?;
                Ok(JoinReport {
                    outcome: "joined".into(),
                    inviter: format!("{name} ({})", id_prefix(&inviter)),
                    registry_doc: root,
                })
            }
            PairReply::SameRoot { name } => {
                self.add_member(&inviter_key, &name).await?;
                self.remember_address(&invite.addr, &inviter).await?;
                Ok(JoinReport {
                    outcome: "same_root".into(),
                    inviter: format!("{name} ({})", id_prefix(&inviter)),
                    registry_doc: root.unwrap_or_default(),
                })
            }
            PairReply::InviterAdopts { name } => {
                self.add_member(&inviter_key, &name).await?;
                self.remember_address(&invite.addr, &inviter).await?;
                let done: PairDone =
                    serde_json::from_str(&read_line(&mut reader, self.options.join_timeout).await?)
                        .context("pairing completion")?;
                if !done.ready {
                    bail!(
                        "{name} could not fetch this server's root: {}",
                        done.error.unwrap_or_default()
                    );
                }
                Ok(JoinReport {
                    outcome: "adopted".into(),
                    inviter: format!("{name} ({})", id_prefix(&inviter)),
                    registry_doc: root.unwrap_or_default(),
                })
            }
        }
    }

    /// Records the join intent, then fetches.
    async fn begin_joining(self: &Arc<Self>, intent: JoinIntent) -> Result<()> {
        let record = intent.clone();
        if !self.db.run(move |db| db.begin_join(&record)).await? {
            return Err(
                AdminError::Invalid("this server already has a root or is joining".into()).into(),
            );
        }
        self.resume_join(intent).await
    }

    /// Fetches the root of a recorded join (also after a restart) until the
    /// whole closure is stored, then records the root.
    pub(crate) async fn resume_join(self: &Arc<Self>, intent: JoinIntent) -> Result<()> {
        let registry = DocumentId::parse_any(&intent.registry_doc)?;
        info!(
            registry = %intent.registry_doc,
            source = %id_prefix(&intent.source_id),
            "joining: fetching the root"
        );
        self.access.set_joining(Some(intent.source_id.clone()));
        let handle = self.repo.find(registry).await?;
        self.install_joining(intent.clone(), handle).await?;
        match &intent.source_addr {
            Some(addr) => self.remember_address(addr, &intent.source_id).await?,
            None => self.ensure_dialer(&intent.source_id),
        }
        let app = Arc::downgrade(self);
        let task = tokio::spawn(complete_join(app, self.access.subscribe_closure()));
        self.tasks.lock().unwrap().push(task);
        Ok(())
    }

    /// Whether the registry and every document reachable from it are
    /// stored; requests what is missing.
    async fn closure_complete(&self) -> Result<bool> {
        let Some(registry) = self.registry_handle() else {
            return Ok(false);
        };
        if registry.status() != DocumentStatus::Ready {
            return Ok(false);
        }
        self.refresh().await?;
        for member in self.access.members() {
            self.refresh_listing(&member.id).await?;
        }
        let mut complete = true;
        for id in self.access.closure() {
            if self.stored(id).await?.is_none() {
                complete = false;
                let _ = self.repo.find(id).await;
            }
        }
        Ok(complete)
    }

    /// Waits until this server is `Ready`.
    pub async fn wait_ready(&self, wait: Duration) -> Result<()> {
        let mut state = self.subscribe_state();
        timeout(wait, state.wait_for(|state| *state == RootState::Ready))
            .await
            .map_err(|_| anyhow!("the join did not complete within {} s", wait.as_secs()))?
            .map_err(|_| anyhow!("the server stopped before the join completed"))?;
        Ok(())
    }
}

async fn complete_join(app: Weak<App>, mut closure: watch::Receiver<u64>) {
    loop {
        let Some(strong) = app.upgrade() else { return };
        match strong.closure_complete().await {
            Ok(true) => {
                if let Err(error) = strong.finish_join().await {
                    warn!(error = %format!("{error:#}"), "recording the joined root failed");
                }
                return;
            }
            Ok(false) => {}
            Err(error) => warn!(error = %format!("{error:#}"), "checking the join failed"),
        }
        drop(strong);
        tokio::select! {
            _ = closure.changed() => {}
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
}

pub const DIFFERENT_ROOTS: &str = "the two servers have different roots; run `tt-server reset` on the one that should join (this deletes its data), then pair again";
pub const BOTH_EMPTY: &str = "neither server has a root; run `tt-server init` on one of them first";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_round_trip() {
        let code = InviteCode {
            addr: "laptop-a.tail1234.ts.net:8772".into(),
            server_id: server_id(&[1; 32]),
            secret: [9; 32],
        };
        let text = code.encode();
        assert!(text.starts_with("tt-pair:"), "{text}");
        assert_eq!(InviteCode::decode(&text).unwrap(), code);
        assert_eq!(InviteCode::decode(&format!("  {text}\n")).unwrap(), code);
        for bad in ["", "tt-pair:", "tt-pair:abc1", "hello", &text[..30]] {
            assert!(InviteCode::decode(bad).is_err(), "{bad}");
        }
        // Another version byte is refused.
        let mut payload = base32_decode(&text[8..]).unwrap();
        payload[0] = 2;
        assert!(InviteCode::decode(&format!("tt-pair:{}", base32(&payload))).is_err());
    }
}
