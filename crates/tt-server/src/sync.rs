//! `GET /sync`: authenticated websocket sessions running the automerge-repo
//! protocol through [`WsJsServer`](automerge_repo::transport::WsJsServer).
//!
//! Every inbound frame passes a per-session gate before the repository sees
//! it. The gate refreshes the ACL cache from the database (so the
//! repository's synchronous policy answers correctly) and applies the
//! new-document rule:
//!
//! - a document the user holds a role on passes (readers may not push
//!   changes);
//! - a document owned by someone else passes and the repository answers
//!   `doc-unavailable`; the attempt is logged;
//! - an unknown document asked for with `request` passes and is answered
//!   `doc-unavailable`;
//! - an unknown document pushed with `sync` is accepted only once the user's
//!   index document lists it: the user becomes its owner and the held frames
//!   are released in order. The index edit usually travels on the same
//!   connection a moment later, so frames wait up to `pending_timeout`;
//!   after that they are discarded and the connection is closed with a
//!   protocol `error`.

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use automerge::sync::Message as SyncMessage;
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
    acl::Access,
    api::{ApiError, authenticate, bearer},
    app::{App, SERVER_PEER_ID, Session},
    auth::{random_hex, secret_hash},
    db::UserRecord,
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
            Ok(Some(grant)) => Ok((grant.user, grant.token_hash)),
            Ok(None) => Err(ApiError::unauthorized()),
            Err(error) => Err(error.into()),
        }
    } else if let Some(token) = bearer(&headers) {
        authenticate(&app, token)
            .await
            .map(|auth| (auth.user, auth.token_hash))
    } else {
        Err(ApiError::unauthorized())
    };
    match granted {
        Ok((user, token_hash)) => {
            ws.on_upgrade(move |socket| session(app, socket, user, token_hash))
        }
        Err(error) => error.into_response(),
    }
}

async fn session(app: Arc<App>, socket: WebSocket, user: UserRecord, token_hash: String) {
    let close = Arc::new(Notify::new());
    let id = app.register(Session {
        token_hash,
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
    user: UserRecord,
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
            WireMessage::Sync {
                document_id, data, ..
            } => self.document(document_id, false, &data, frame).await,
            WireMessage::Request {
                document_id, data, ..
            } => self.document(document_id, true, &data, frame).await,
            _ => self.forward(frame).await,
        }
    }

    async fn document(
        &mut self,
        id: DocumentId,
        request: bool,
        data: &[u8],
        frame: Vec<u8>,
    ) -> Result<(), Reject> {
        if let Some(held) = self.pending.get_mut(&id) {
            held.frames.push(frame);
            return Ok(());
        }
        match self.app.acl.access(id, &self.user.id).await? {
            Access::Granted(role) => {
                if !request && !role.may_write() && carries_changes(data) {
                    warn!(user = %self.user.name, document = %id.to_bs58check(), "write to a read-only document");
                    return Err(Reject(format!(
                        "document {} is read-only for this user",
                        id.to_bs58check()
                    )));
                }
                self.forward(frame).await
            }
            Access::Foreign => {
                warn!(
                    user = %self.user.name,
                    document = %id.to_bs58check(),
                    "refused another user's document"
                );
                self.forward(frame).await
            }
            Access::Unknown if request => self.forward(frame).await,
            Access::Unknown => {
                let listed = self
                    .app
                    .index_listing(&self.user.index_doc)
                    .await?
                    .contains(&id);
                if listed {
                    self.claim(id).await?;
                    self.forward(frame).await
                } else {
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
    }

    async fn claim(&self, id: DocumentId) -> Result<(), Reject> {
        match self.app.acl.claim(id, &self.user.id).await? {
            Access::Granted(_) => {
                info!(user = %self.user.name, document = %id.to_bs58check(), "accepted new document");
            }
            _ => {
                warn!(user = %self.user.name, document = %id.to_bs58check(), "new document already owned by another user");
            }
        }
        Ok(())
    }

    /// Releases held documents the index now lists.
    async fn release(&mut self) -> Result<(), Reject> {
        let listing = self.app.index_listing(&self.user.index_doc).await?;
        let ready: Vec<DocumentId> = self
            .pending
            .keys()
            .filter(|id| listing.contains(id))
            .copied()
            .collect();
        for id in ready {
            self.claim(id).await?;
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
            sender_id: SERVER_PEER_ID.into(),
            target_id: self.wire_id.clone(),
            message: reason,
        };
        if let Ok(frame) = error.encode() {
            let _ = self.outgoing.send(frame).await;
        }
    }
}

/// Whether a sync payload carries changes (rather than only heads, needs,
/// and bloom filters).
fn carries_changes(data: &[u8]) -> bool {
    SyncMessage::decode(data).map_or(true, |message| !message.changes.is_empty())
}
