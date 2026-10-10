//! `GET /sync`: authenticated websocket sessions running the automerge-repo
//! protocol through [`WsJsServer`](automerge_repo::transport::WsJsServer).
//!
//! Every inbound frame passes a per-session gate before the repository sees
//! it. The gate re-reads the user's index into the access index when a
//! document is not yet granted (so the repository's synchronous policy
//! answers correctly) and applies the new-document rule:
//!
//! - a document the user owns (its index, or one its index lists) passes;
//! - a document owned by someone else, or the registry, passes and the
//!   repository answers `doc-unavailable`; the attempt is logged;
//! - an unlisted document asked for with `request` passes and is answered
//!   `doc-unavailable`;
//! - an unlisted document pushed with `sync` is accepted only once the
//!   user's index document lists it; the held frames are then released in
//!   order. The index edit usually travels on the same connection a moment
//!   later, so frames wait up to `pending_timeout`; after that they are
//!   discarded and the connection is closed with a protocol `error`.

use std::{collections::HashMap, sync::Arc, time::Duration};

use automerge_repo::{DocumentId, protocol::WireMessage};
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt, stream::SplitStream};
use tokio::{
    sync::{Notify, mpsc},
    time::{Instant, sleep_until},
};
use tracing::{debug, info, warn};

use crate::{
    access::Access,
    api::{ApiError, TokenRef, authenticate, bearer, sync_origin_allowed, token_grant},
    app::{App, Session},
    auth::{random_hex, secret_hash},
    registry::Account,
};

const FRAME_QUEUE: usize = 256;
/// How often held documents are checked against the user's index.
const RECHECK: Duration = Duration::from_millis(100);

pub(crate) async fn upgrade(
    State(app): State<Arc<App>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Browsers always send `Origin`; other clients (daemon, peers) may not.
    if let Some(origin) = headers.get(axum::http::header::ORIGIN) {
        let allowed = origin
            .to_str()
            .is_ok_and(|origin| sync_origin_allowed(&app, origin, &headers));
        if !allowed {
            warn!(origin = ?origin, "refused a websocket upgrade from an unknown origin");
            return ApiError::new(StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }
    if query.contains_key("token") {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "tokens are never accepted in URLs; get a ticket from /api/ws-ticket",
        )
        .into_response();
    }
    let granted = if let Some(ticket) = query.get("ticket") {
        let hash = secret_hash(ticket);
        match app.db.run(move |db| db.consume_ticket(&hash)).await {
            Ok(Some(grant)) => {
                match token_grant(&app.view(), &grant.user_id, &grant.issuer, &grant.token_id) {
                    Some(user) => Ok((
                        user,
                        TokenRef {
                            id: grant.token_id,
                            issuer: grant.issuer,
                        },
                    )),
                    None => Err(ApiError::unauthorized()),
                }
            }
            Ok(None) => Err(ApiError::unauthorized()),
            Err(error) => Err(error.into()),
        }
    } else if let Some(token) = bearer(&headers) {
        authenticate(&app, token)
            .await
            .map(|auth| (auth.user, auth.token))
    } else {
        Err(ApiError::unauthorized())
    };
    match granted {
        Ok((user, token)) => ws.on_upgrade(move |socket| session(app, socket, user, token)),
        Err(error) => error.into_response(),
    }
}

async fn session(app: Arc<App>, socket: WebSocket, user: Account, token: TokenRef) {
    let close = Arc::new(Notify::new());
    let id = app.register(Session {
        token,
        user_id: user.id.clone(),
        close: close.clone(),
    });
    // The repository sees `<user id>.<nonce>/<senderId>`: two devices of one
    // user never collide, and a client cannot pick another user's identity.
    let identity = format!("{}.{}", user.id, random_hex(8));
    debug!(user = %user.name, %identity, "sync session open");
    let (mut sink, stream) = socket.split();
    let (incoming_tx, incoming) = mpsc::channel::<Vec<u8>>(FRAME_QUEUE);
    let (outgoing, mut outgoing_rx) = mpsc::channel::<Vec<u8>>(FRAME_QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(frame) = outgoing_rx.recv().await {
            if sink.send(Message::Binary(frame.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let gate = Gate {
        app: app.clone(),
        user,
        wire_id: None,
        outgoing: outgoing.clone(),
        incoming: incoming_tx,
        pending: HashMap::new(),
    };
    let reader = tokio::spawn(gate.run(stream, close));
    if let Err(error) = app
        .transport
        .clients()
        .serve_frames(incoming, outgoing, Some(identity))
        .await
    {
        debug!(%error, "sync session ended with a protocol error");
    }
    reader.abort();
    let _ = reader.await;
    let _ = writer.await;
    app.unregister(id);
}

struct Held {
    deadline: Instant,
    frames: Vec<Vec<u8>>,
}

struct Gate {
    app: Arc<App>,
    user: Account,
    /// The client's `senderId`, from its `join`.
    wire_id: Option<String>,
    outgoing: mpsc::Sender<Vec<u8>>,
    incoming: mpsc::Sender<Vec<u8>>,
    pending: HashMap<DocumentId, Held>,
}

/// Ends the session; the reason is sent to the client as a protocol `error`.
struct Reject(String);

impl From<anyhow::Error> for Reject {
    fn from(error: anyhow::Error) -> Self {
        warn!(error = %format!("{error:#}"), "sync gate failed");
        Self("internal error".into())
    }
}

async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

impl Gate {
    async fn run(mut self, mut stream: SplitStream<WebSocket>, close: Arc<Notify>) {
        let mut recheck = tokio::time::interval(RECHECK);
        loop {
            let deadline = self.pending.values().map(|held| held.deadline).min();
            let outcome = tokio::select! {
                frame = stream.next() => match frame {
                    Some(Ok(Message::Binary(bytes))) => self.inbound(bytes.to_vec()).await,
                    Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                    Some(Ok(_)) => Ok(()),
                },
                () = close.notified() => Err(Reject("token revoked".into())),
                _ = recheck.tick(), if !self.pending.is_empty() => self.release().await,
                () = until(deadline) => self.expire(),
            };
            if let Err(Reject(reason)) = outcome {
                self.reject(reason).await;
                return;
            }
        }
    }

    async fn forward(&self, frame: Vec<u8>) -> Result<(), Reject> {
        self.incoming
            .send(frame)
            .await
            .map_err(|_| Reject("session closed".into()))
    }

    async fn inbound(&mut self, frame: Vec<u8>) -> Result<(), Reject> {
        // Undecodable frames are the repository's to reject.
        let Ok(message) = WireMessage::decode(&frame) else {
            return self.forward(frame).await;
        };
        match message {
            WireMessage::Join { sender_id, .. } => {
                self.wire_id = Some(sender_id);
                self.forward(frame).await
            }
            WireMessage::Sync { document_id, .. } => self.document(document_id, false, frame).await,
            WireMessage::Request { document_id, .. } => {
                self.document(document_id, true, frame).await
            }
            _ => self.forward(frame).await,
        }
    }

    async fn document(
        &mut self,
        id: DocumentId,
        request: bool,
        frame: Vec<u8>,
    ) -> Result<(), Reject> {
        if let Some(held) = self.pending.get_mut(&id) {
            held.frames.push(frame);
            return Ok(());
        }
        match self.app.access_for(id, &self.user.id).await? {
            Access::Granted => self.forward(frame).await,
            Access::Foreign => {
                warn!(
                    user = %self.user.name,
                    document = %id.to_bs58check(),
                    "refused a document the user does not own"
                );
                self.forward(frame).await
            }
            Access::Unlisted if request => self.forward(frame).await,
            Access::Unlisted => {
                self.pending.insert(
                    id,
                    Held {
                        deadline: Instant::now() + self.app.options.pending_timeout,
                        frames: vec![frame],
                    },
                );
                Ok(())
            }
        }
    }

    /// Releases held documents the index now lists.
    async fn release(&mut self) -> Result<(), Reject> {
        self.app.refresh_listing(&self.user.id).await?;
        let ready: Vec<(DocumentId, Access)> = self
            .pending
            .keys()
            .map(|id| (*id, self.app.access.access(*id, &self.user.id)))
            .filter(|(_, access)| *access != Access::Unlisted)
            .collect();
        for (id, access) in ready {
            if access == Access::Granted {
                info!(user = %self.user.name, document = %id.to_bs58check(), "accepted new document");
            } else {
                warn!(user = %self.user.name, document = %id.to_bs58check(), "new document is owned by another user");
            }
            if let Some(held) = self.pending.remove(&id) {
                for frame in held.frames {
                    self.forward(frame).await?;
                }
            }
        }
        Ok(())
    }

    fn expire(&mut self) -> Result<(), Reject> {
        let now = Instant::now();
        match self.pending.iter().find(|(_, held)| held.deadline <= now) {
            Some((id, _)) => {
                warn!(
                    user = %self.user.name,
                    document = %id.to_bs58check(),
                    "discarded a document the user's index does not list"
                );
                Err(Reject(format!(
                    "document {} is not listed in your index document",
                    id.to_bs58check()
                )))
            }
            None => Ok(()),
        }
    }

    async fn reject(&mut self, reason: String) {
        self.pending.clear();
        let error = WireMessage::Error {
            sender_id: self.app.peer_id().to_string(),
            target_id: self.wire_id.clone(),
            message: reason,
        };
        if let Ok(frame) = error.encode() {
            let _ = self.outgoing.send(frame).await;
        }
    }
}
