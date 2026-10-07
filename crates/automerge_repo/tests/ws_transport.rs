//! Websocket transports against scripted peers: version negotiation,
//! reconnection, and server-side identity scoping.

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use automerge::{Automerge, ROOT, sync::SyncDoc, transaction::Transactable};
use automerge_repo::{
    DocumentId, Error, PeerId, Repo, RepoConfig,
    error::ProtocolError,
    network::{NetworkEvent, NetworkTransport},
    protocol::{PeerMetadata, WireMessage},
    testing::MemoryStore,
    transport::{ConnectionState, WsJsClient, WsJsClientConfig, WsJsServer},
};
use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message as WsMessage};

const STEP: Duration = Duration::from_secs(10);

async fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(STEP, future)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

async fn open_repo(transport: Arc<dyn NetworkTransport>) -> Repo {
    Repo::open(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryStore::default()),
        transport,
        RepoConfig::default(),
    )
    .await
    .unwrap()
}

fn join(sender: &str, versions: &[&str]) -> Vec<u8> {
    WireMessage::Join {
        sender_id: sender.into(),
        peer_metadata: PeerMetadata::default(),
        supported_protocol_versions: versions.iter().map(|&version| version.into()).collect(),
    }
    .encode()
    .unwrap()
}

fn peer(sender: &str, target: &str, version: &str) -> Vec<u8> {
    WireMessage::Peer {
        sender_id: sender.into(),
        target_id: target.into(),
        selected_protocol_version: version.into(),
        peer_metadata: PeerMetadata::default(),
    }
    .encode()
    .unwrap()
}

/// Next binary frame, decoded; `None` once the socket closes.
async fn next_message<S>(socket: &mut WebSocketStream<S>) -> Option<WireMessage>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        match socket.next().await? {
            Ok(WsMessage::Binary(bytes)) => return Some(WireMessage::decode(&bytes).unwrap()),
            Ok(WsMessage::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// Fake server half of the handshake: reads `join`, answers `peer` with
/// `version`, and returns the client's `senderId`.
async fn accept_and_answer(
    listener: &TcpListener,
    version: &str,
) -> (WebSocketStream<TcpStream>, String) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
    let Some(WireMessage::Join { sender_id, .. }) = next_message(&mut socket).await else {
        panic!("client must open with join");
    };
    socket
        .send(WsMessage::Binary(
            peer("fake-server", &sender_id, version).into(),
        ))
        .await
        .unwrap();
    (socket, sender_id)
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    (listener, url)
}

fn record<T: Clone + Send + Sync + 'static>(
    mut receiver: tokio::sync::watch::Receiver<T>,
) -> Arc<Mutex<Vec<T>>> {
    let seen = Arc::new(Mutex::new(vec![receiver.borrow_and_update().clone()]));
    let sink = seen.clone();
    tokio::spawn(async move {
        while receiver.changed().await.is_ok() {
            let value = receiver.borrow_and_update().clone();
            sink.lock().unwrap().push(value);
        }
    });
    seen
}

#[tokio::test]
async fn client_fails_permanently_and_closes_on_protocol_version_mismatch() {
    let (listener, url) = listener().await;
    let client = WsJsClient::start(WsJsClientConfig::new(url, "rust-client"));
    let (mut socket, sender) = within("first connection", accept_and_answer(&listener, "2")).await;
    assert_eq!(sender, "rust-client");
    // The client closes the socket rather than lingering.
    within("client close", async {
        while next_message(&mut socket).await.is_some() {}
    })
    .await;
    let mut state = client.subscribe_state();
    let failed = within(
        "failed state",
        state.wait_for(|state| matches!(state, ConnectionState::Failed(_))),
    )
    .await
    .unwrap()
    .clone();
    assert_eq!(
        failed,
        ConnectionState::Failed(ProtocolError::VersionMismatch {
            selected: "2".into()
        })
    );
    // A version mismatch is not retried.
    assert!(
        tokio::time::timeout(Duration::from_millis(500), listener.accept())
            .await
            .is_err(),
        "client must not reconnect after a version mismatch"
    );
    assert_eq!(client.state(), failed);
}

#[tokio::test]
async fn client_reconnects_with_backoff_after_the_server_drops_the_socket() {
    let (listener, url) = listener().await;
    let mut config = WsJsClientConfig::new(url, "rust-client");
    config.min_backoff = Duration::from_millis(100);
    config.max_backoff = Duration::from_secs(1);
    let client = WsJsClient::start(config);
    let states = record(client.subscribe_state());
    let repo = open_repo(client.clone()).await;
    let peers = record(repo.subscribe_peers());
    let document = repo
        .create_with(|tx| {
            tx.put(ROOT, "title", "survives reconnects")
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();

    // First session: complete the handshake, then drop the socket.
    let (mut first, _) = within("first connection", accept_and_answer(&listener, "1")).await;
    let mut first_peers = repo.subscribe_peers();
    within(
        "first peer",
        first_peers.wait_for(|peers| peers == &[PeerId::from("fake-server")]),
    )
    .await
    .unwrap();
    first.close(None).await.unwrap();
    drop(first);

    // Second session: the repository announces its document afresh.
    let (mut second, sender) = within("reconnection", accept_and_answer(&listener, "1")).await;
    assert_eq!(sender, "rust-client");
    let announced = within("announcement on the new session", async {
        loop {
            match next_message(&mut second).await {
                Some(WireMessage::Sync {
                    document_id,
                    target_id,
                    ..
                }) if document_id == document.id() => break target_id,
                Some(_) => {}
                None => panic!("second session closed early"),
            }
        }
    })
    .await;
    assert_eq!(announced, "fake-server");

    let mut state = client.subscribe_state();
    within(
        "connected again",
        state.wait_for(|state| matches!(state, ConnectionState::Connected { .. })),
    )
    .await
    .unwrap();
    let states = states.lock().unwrap().clone();
    let connected = |state: &ConnectionState| matches!(state, ConnectionState::Connected { .. });
    let first_up = states.iter().position(connected).unwrap();
    let down = first_up
        + states[first_up..]
            .iter()
            .position(|state| matches!(state, ConnectionState::Disconnected { .. }))
            .unwrap_or_else(|| panic!("no Disconnected after Connected: {states:?}"));
    let ConnectionState::Disconnected { retry_in, .. } = &states[down] else {
        unreachable!()
    };
    assert_eq!(*retry_in, Duration::from_millis(100));
    assert!(
        states[down..].iter().any(connected),
        "no Connected after Disconnected: {states:?}"
    );

    let peers = peers.lock().unwrap().clone();
    let up = |list: &Vec<PeerId>| list == &[PeerId::from("fake-server")];
    let first_up = peers.iter().position(up).unwrap();
    let gone = first_up
        + peers[first_up..]
            .iter()
            .position(Vec::is_empty)
            .unwrap_or_else(|| panic!("peer never dropped: {peers:?}"));
    assert!(
        peers[gone..].iter().any(up),
        "no fresh PeerConnected: {peers:?}"
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn server_answers_unsupported_versions_with_error_and_closes() {
    let server = WsJsServer::new("rust-server");
    let mut events = server.take_events().unwrap();
    let (listener, url) = listener().await;
    tokio::spawn(server.clone().listen(listener));
    let (mut socket, _): (WebSocketStream<MaybeTlsStream<TcpStream>>, _) =
        tokio_tungstenite::connect_async(url).await.unwrap();
    socket
        .send(WsMessage::Binary(join("js-client", &["2", "3"]).into()))
        .await
        .unwrap();
    let reply = within("error reply", next_message(&mut socket)).await;
    let Some(WireMessage::Error {
        sender_id,
        target_id,
        message,
    }) = reply
    else {
        panic!("expected error, got {reply:?}");
    };
    assert_eq!(sender_id, "rust-server");
    assert_eq!(target_id.as_deref(), Some("js-client"));
    assert!(
        message.contains("unsupported protocol version"),
        "{message}"
    );
    assert!(
        within("server close", next_message(&mut socket))
            .await
            .is_none(),
        "server must close after the error"
    );
    assert!(
        events.try_recv().is_err(),
        "a rejected join never reaches the repository"
    );
}

#[tokio::test]
async fn server_identity_scopes_repository_peer_ids_and_retargets_frames() {
    let server = WsJsServer::new("rust-server");
    let repo = open_repo(server.clone()).await;
    let document = repo
        .create_with(|tx| {
            tx.put(ROOT, "owner", "alice")
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();

    let mut sessions = Vec::new();
    for identity in ["alice", "bob"] {
        let (in_tx, in_rx) = mpsc::channel(16);
        let (out_tx, out_rx) = mpsc::channel(64);
        let session = tokio::spawn({
            let server = server.clone();
            async move {
                server
                    .serve_frames(in_rx, out_tx, Some(identity.into()))
                    .await
            }
        });
        in_tx.send(join("client-1", &["1"])).await.unwrap();
        sessions.push((identity, in_tx, out_rx, session));
    }
    let mut peers = repo.subscribe_peers();
    within(
        "both identities",
        peers.wait_for(|peers| {
            peers == &[PeerId::from("alice/client-1"), PeerId::from("bob/client-1")]
        }),
    )
    .await
    .unwrap();

    let unknown = DocumentId::new();
    for (identity, in_tx, out_rx, _) in &mut sessions {
        let Some(handshake) = out_rx.recv().await else {
            panic!("{identity}: no handshake reply");
        };
        assert_eq!(
            WireMessage::decode(&handshake).unwrap(),
            WireMessage::Peer {
                sender_id: "rust-server".into(),
                target_id: "client-1".into(),
                selected_protocol_version: "1".into(),
                peer_metadata: PeerMetadata {
                    storage_id: None,
                    is_ephemeral: Some(false),
                },
            }
        );
        // Every repository frame for this session is retargeted to the
        // wire id the client knows itself by.
        let announced = within("announcement", async {
            loop {
                let frame = WireMessage::decode(&out_rx.recv().await.unwrap()).unwrap();
                if let WireMessage::Sync {
                    sender_id,
                    target_id,
                    document_id,
                    ..
                } = frame
                {
                    break (sender_id, target_id, document_id);
                }
            }
        })
        .await;
        assert_eq!(
            announced,
            ("rust-server".into(), "client-1".into(), document.id())
        );

        let mut state = automerge::sync::State::new();
        let data = Automerge::new()
            .generate_sync_message(&mut state)
            .unwrap()
            .encode();
        in_tx
            .send(
                WireMessage::Request {
                    sender_id: "client-1".into(),
                    target_id: "rust-server".into(),
                    document_id: unknown,
                    data,
                }
                .encode()
                .unwrap(),
            )
            .await
            .unwrap();
        let unavailable = within("doc-unavailable", async {
            loop {
                let frame = WireMessage::decode(&out_rx.recv().await.unwrap()).unwrap();
                if let WireMessage::DocUnavailable { .. } = frame {
                    break frame;
                }
            }
        })
        .await;
        assert_eq!(
            unavailable,
            WireMessage::DocUnavailable {
                sender_id: "rust-server".into(),
                target_id: "client-1".into(),
                document_id: unknown,
            }
        );
    }
    assert!(
        repo.peer_sync_progress()
            .contains_key(&PeerId::from("alice/client-1"))
    );

    // Ending one session disconnects only that identity.
    let (_, alice_in, _, alice_session) = sessions.remove(0);
    drop(alice_in);
    within("alice session end", alice_session)
        .await
        .unwrap()
        .unwrap();
    within(
        "alice gone",
        peers.wait_for(|peers| peers == &[PeerId::from("bob/client-1")]),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn server_without_identity_uses_the_wire_sender_id() {
    let server = WsJsServer::new("rust-server");
    let mut events = server.take_events().unwrap();
    let (in_tx, in_rx) = mpsc::channel(4);
    let (out_tx, mut out_rx) = mpsc::channel(4);
    let session = tokio::spawn({
        let server = server.clone();
        async move { server.serve_frames(in_rx, out_tx, None).await }
    });
    in_tx.send(join("plain", &[])).await.unwrap();
    assert!(matches!(
        WireMessage::decode(&out_rx.recv().await.unwrap()).unwrap(),
        WireMessage::Peer { target_id, .. } if target_id == "plain"
    ));
    assert_eq!(
        within("connect event", events.recv()).await,
        Some(NetworkEvent::PeerConnected(PeerId::from("plain")))
    );
    server.close_peer(&PeerId::from("plain")).await.unwrap();
    within("session end", session).await.unwrap().unwrap();
    assert_eq!(
        within("disconnect event", events.recv()).await,
        Some(NetworkEvent::PeerDisconnected(PeerId::from("plain")))
    );
}
