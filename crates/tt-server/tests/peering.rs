//! Peering between servers: pairing and root direction, key-pinned links
//! next to a plain-HTTP client listener, trust and revocation, the access
//! rules for server peers, the registry-driven inventory, crash-safe joins,
//! and reset.

mod common;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{DocHandle, DocumentId, Error};
use common::{Client, Proxy, TestServer, eventually};
use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;
use tt_server::{RootState, admin, db::Db};

async fn put(handle: &DocHandle, key: &'static str, value: &'static str) {
    handle
        .change(move |tx| {
            tx.put(ROOT, key, value)
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();
}

async fn get(handle: &DocHandle, key: &'static str) -> Option<String> {
    handle
        .read(move |doc| {
            doc.get(ROOT, key)
                .unwrap()
                .and_then(|(value, _)| value.into_string().ok())
        })
        .await
        .unwrap()
}

/// A stored document's value of `key` on a server.
async fn stored_value(server: &TestServer, doc: &str, key: &'static str) -> Option<String> {
    let id = DocumentId::parse_any(doc).unwrap();
    let handle = server.server().repo().open_document(id).await.ok()?;
    get(&handle, key).await
}

/// Whether a server stores a document with a tt schema kind.
async fn has_doc(server: &TestServer, doc: &str) -> bool {
    let id = DocumentId::parse_any(doc).unwrap();
    match server.server().repo().open_document(id).await {
        Ok(handle) => handle.read(tt_core::schema::kind).await.unwrap().is_some(),
        Err(_) => false,
    }
}

/// Member names as `peer ls` reports them.
async fn members(server: &TestServer) -> Vec<String> {
    let mut names: Vec<String> = admin::list_servers(&server.db())
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| !entry.is_revoked())
        .map(|entry| entry.name)
        .collect();
    names.sort();
    names
}

/// A server with the peer listener on, initialized as `name` or empty.
async fn peer_server(init: Option<&str>) -> TestServer {
    let mut server = match init {
        Some(name) => {
            let dir = tempfile::tempdir().unwrap();
            admin::init(&dir.path().join("server.db"), Some(name))
                .await
                .unwrap();
            TestServer::reopen(dir, |_| {}).await
        }
        None => TestServer::start_uninitialized(|_| {}).await,
    };
    server.listen_peers().await;
    server
}

async fn join(
    server: &TestServer,
    code: &str,
    name: &str,
) -> anyhow::Result<tt_server::peer::JoinReport> {
    admin::join(&server.db(), code, Some(name)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_server_joins_over_the_peer_port_next_to_plain_http() {
    // The client listeners of both servers are plain HTTP (as behind nginx);
    // the peer port does its own TLS.
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    let login = a.login("alice").await;
    let client = Client::connect(&a, &login, "phone").await;
    let workspace = client.find_ready(&alice.workspace_doc).await;
    put(&workspace, "note", "written on a").await;
    eventually("a stores the note", 10, || async {
        stored_value(&a, &alice.workspace_doc, "note")
            .await
            .as_deref()
            == Some("written on a")
    })
    .await;

    let b = peer_server(None).await;
    assert_eq!(b.app().state(), RootState::NeedsDecision);
    let code = a.invite().await;
    let report = join(&b, &code, "laptop-b").await.unwrap();
    assert_eq!(report.outcome, "joined");
    assert_eq!(b.app().state(), RootState::Ready);
    assert_eq!(
        b.app().root().unwrap().registry_doc,
        a.app().root().unwrap().registry_doc
    );
    b.wait_linked(&a, 10).await;
    a.wait_linked(&b, 10).await;
    assert_eq!(members(&a).await, ["laptop-a", "laptop-b"]);
    eventually("b lists both members", 10, || async {
        members(&b).await == ["laptop-a", "laptop-b"]
    })
    .await;

    // A client logging into B sees the same data.
    let on_b = b.login("alice").await;
    let client_b = Client::connect(&b, &on_b, "laptop").await;
    let mirror = client_b.find_ready(&alice.workspace_doc).await;
    assert_eq!(get(&mirror, "note").await.as_deref(), Some("written on a"));

    // Changes flow both ways while linked.
    put(&mirror, "reply", "written on b").await;
    eventually("a gets b's change", 10, || async {
        get(&workspace, "reply").await.as_deref() == Some("written on b")
    })
    .await;
}

async fn invite_at(server: &TestServer, addr: SocketAddr) -> String {
    let mut code = String::new();
    admin::invite(
        &server.db(),
        Some(&addr.to_string()),
        None,
        None,
        |invitation| {
            code = invitation.code.clone();
        },
    )
    .await
    .unwrap();
    code
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rooted_server_joins_an_empty_inviter_which_adopts_its_root() {
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    let cloud = peer_server(None).await;
    // The empty public server invites; the rooted laptop runs `join`.
    let mut code = String::new();
    admin::invite(&cloud.db(), None, None, Some("cloud"), |invitation| {
        code = invitation.code.clone();
    })
    .await
    .unwrap();
    let report = join(&a, &code, "ignored").await.unwrap();
    assert_eq!(report.outcome, "adopted");
    assert_eq!(cloud.app().state(), RootState::Ready);
    assert_eq!(
        cloud.app().root().unwrap().registry_doc,
        a.app().root().unwrap().registry_doc
    );
    assert_eq!(cloud.app().root().unwrap().name, "cloud");
    assert_eq!(members(&a).await, ["cloud", "laptop-a"]);
    eventually("the cloud lists both", 10, || async {
        members(&cloud).await == ["cloud", "laptop-a"]
    })
    .await;
    // The cloud serves alice now.
    let login = cloud.login("alice").await;
    let client = Client::connect(&cloud, &login, "phone").await;
    client.find_ready(&alice.workspace_doc).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_same_roots_only_refreshes_membership() {
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    let b = peer_server(None).await;
    join(&b, &a.invite().await, "laptop-b").await.unwrap();
    assert!(has_doc(&b, &alice.workspace_doc).await);
    let report = join(&b, &a.invite().await, "laptop-b2").await.unwrap();
    assert_eq!(report.outcome, "same_root");
    assert_eq!(
        b.app().root().unwrap().registry_doc,
        a.app().root().unwrap().registry_doc,
        "no root is replaced"
    );
    assert!(has_doc(&b, &alice.workspace_doc).await);
    // The inviter refreshed B's name; B refreshed A's.
    assert_eq!(members(&a).await, ["laptop-a", "laptop-b2"]);
    eventually("b sees its new name", 10, || async {
        members(&b).await == ["laptop-a", "laptop-b2"]
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn different_roots_are_refused_until_reset() {
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    let x = peer_server(Some("laptop-x")).await;
    let xavier = x.add_user("xavier").await;
    let code = a.invite().await;
    let error = join(&x, &code, "laptop-x").await.unwrap_err().to_string();
    assert!(error.contains("tt-server reset"), "{error}");
    assert_eq!(members(&a).await, ["laptop-a"], "the inviter is unchanged");
    assert_eq!(members(&x).await, ["laptop-x"], "the joiner is unchanged");
    let x_id = x.server_id();

    // `reset` (with the server stopped), then the same code works: the code
    // was not used by the refused attempt.
    let dir = x.stop().await;
    let db = dir.path().join("server.db");
    admin::reset(&db, false).await.unwrap();
    let mut x = TestServer::reopen(dir, |_| {}).await;
    x.listen_peers().await;
    assert_eq!(x.app().state(), RootState::NeedsDecision);
    assert_eq!(x.server_id(), x_id, "reset keeps the identity");
    join(&x, &code, "laptop-x").await.unwrap();
    assert_eq!(
        x.app().root().unwrap().registry_doc,
        a.app().root().unwrap().registry_doc
    );
    assert!(has_doc(&x, &alice.workspace_doc).await);
    let gone = DocumentId::parse_any(&xavier.workspace_doc).unwrap();
    assert!(
        x.server().repo().open_document(gone).await.is_err(),
        "no documents from the old root"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_empty_servers_cannot_pair() {
    let a = peer_server(None).await;
    let b = peer_server(None).await;
    let error = join(&b, &a.invite().await, "b")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("tt-server init"), "{error}");
    assert_eq!(a.app().state(), RootState::NeedsDecision);
    assert_eq!(b.app().state(), RootState::NeedsDecision);
}

#[tokio::test(flavor = "multi_thread")]
async fn codes_are_single_use_expire_and_pin_the_inviter_key() {
    use tt_server::peer::InviteCode;
    let a = peer_server(Some("laptop-a")).await;
    let b = peer_server(None).await;
    let c = peer_server(None).await;

    // A code naming another server's key, at A's address: aborted before
    // the secret is sent, so the code stays usable.
    let code = a.invite().await;
    let mut forged = InviteCode::decode(&code).unwrap();
    forged.server_id = c.server_id();
    let error = join(&b, &forged.encode(), "b")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("nothing was sent"), "{error}");
    let hash = tt_server::auth::secret_hash(&tt_server::registry::hex(&forged.secret));
    assert_eq!(
        Db::open(&a.db()).unwrap().invite_state(&hash).unwrap(),
        tt_server::db::InviteState::Valid
    );

    // Used once, refused the second time with nothing changed.
    join(&b, &code, "laptop-b").await.unwrap();
    let error = join(&c, &code, "laptop-c").await.unwrap_err().to_string();
    assert!(error.contains("already used"), "{error}");
    assert_eq!(c.app().state(), RootState::NeedsDecision);
    assert_eq!(members(&a).await, ["laptop-a", "laptop-b"]);

    // An expired code.
    let expired = InviteCode {
        addr: a.peer_addr.unwrap().to_string(),
        server_id: a.server_id(),
        secret: [7; 32],
    };
    let hash = tt_server::auth::secret_hash(&tt_server::registry::hex(&expired.secret));
    Db::open(&a.db())
        .unwrap()
        .insert_invite(&hash, tt_server::db::now_ms() - 1, None)
        .unwrap();
    let error = join(&c, &expired.encode(), "laptop-c")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("expired"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_members_link_and_revocation_closes_links() {
    let a = peer_server(Some("laptop-a")).await;
    let b = peer_server(None).await;
    let c = peer_server(None).await;
    join(&b, &a.invite().await, "nixos").await.unwrap();
    join(&c, &a.invite().await, "nixos").await.unwrap();
    // Two members link to one server at once.
    a.wait_linked(&b, 10).await;
    a.wait_linked(&c, 10).await;

    // A stranger with its own root, and a server dialing A's client port,
    // never link.
    let stranger = peer_server(Some("stranger")).await;
    stranger.server().add_peer(a.peer_addr.unwrap().to_string());
    stranger.server().add_peer(a.addr.to_string());
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(stranger.app().linked_servers().is_empty());
    assert!(!a.app().linked_servers().contains(&stranger.server_id()));

    // Duplicate names: distinct prefixes, and the name alone is ambiguous.
    let listed = admin::list_servers(&a.db()).await.unwrap();
    let nixos: Vec<String> = listed
        .iter()
        .filter(|entry| entry.name == "nixos")
        .map(|entry| entry.display())
        .collect();
    assert_eq!(nixos.len(), 2);
    assert_ne!(nixos[0], nixos[1]);
    let error = admin::revoke_server(&a.db(), "nixos")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("id prefix"), "{error}");
    let error = admin::revoke_server(&a.db(), &a.server_id()[..8])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("tt-server reset"), "{error}");

    // Revoking B closes the link within 5 seconds, and B stays out.
    let revoked = admin::revoke_server(&a.db(), &b.server_id()[..8])
        .await
        .unwrap();
    assert!(revoked.is_revoked());
    let b_id = b.server_id();
    let mut links = a.app().subscribe_links();
    timeout(
        Duration::from_secs(5),
        links.wait_for(|linked| !linked.contains(&b_id)),
    )
    .await
    .expect("the revoked member's link closes within 5 s")
    .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!a.app().linked_servers().contains(&b_id));
    // C learns about the revocation and refuses B too.
    eventually("c sees b revoked", 10, || async {
        admin::list_servers(&c.db())
            .await
            .unwrap()
            .iter()
            .any(|entry| entry.id == b_id && entry.is_revoked())
    })
    .await;
    assert!(a.app().linked_servers().contains(&c.server_id()));
}

/// A hand-rolled member: TLS with a member's key, then raw protocol frames.
struct RawPeer {
    socket:
        tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
}

impl RawPeer {
    async fn connect(key_file: &std::path::Path, addr: SocketAddr) -> Self {
        use automerge_repo::protocol::{PROTOCOL_VERSION, PeerMetadata, WireMessage};
        let identity = tt_server::identity::Identity::load_or_create(key_file).unwrap();
        let tls = tt_server::tls::PeerTls::new(&identity).unwrap();
        let config = tls
            .client(tt_server::tls::ALPN_SYNC, Arc::new(|_: &[u8]| true))
            .unwrap();
        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (stream, _) = tt_server::tls::connect(tcp, config).await.unwrap();
        let (mut socket, _) = tokio_tungstenite::client_async("ws://tt-peer/", stream)
            .await
            .unwrap();
        let join = WireMessage::Join {
            sender_id: "srv:raw".into(),
            peer_metadata: PeerMetadata::default(),
            supported_protocol_versions: vec![PROTOCOL_VERSION.into()],
        };
        socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                join.encode().unwrap().into(),
            ))
            .await
            .unwrap();
        let mut peer = Self { socket };
        match peer.next().await {
            Some(WireMessage::Peer { .. }) => peer,
            other => panic!("expected peer, got {other:?}"),
        }
    }

    async fn send(&mut self, message: automerge_repo::protocol::WireMessage) {
        self.socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                message.encode().unwrap().into(),
            ))
            .await
            .unwrap();
    }

    /// The next protocol message; `None` once closed.
    async fn next(&mut self) -> Option<automerge_repo::protocol::WireMessage> {
        loop {
            match timeout(Duration::from_secs(10), self.socket.next())
                .await
                .expect("a frame within 10 s")
            {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) => {
                    return Some(automerge_repo::protocol::WireMessage::decode(&bytes).unwrap());
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_)) | None => {
                    return None;
                }
                Some(Ok(_)) => {}
            }
        }
    }
}

fn sync_data() -> Vec<u8> {
    use automerge::sync::SyncDoc;
    let mut doc = automerge::AutoCommit::new();
    doc.put(ROOT, "x", 1).unwrap();
    let mut state = automerge::sync::State::new();
    doc.sync()
        .generate_sync_message(&mut state)
        .unwrap()
        .encode()
}

#[tokio::test(flavor = "multi_thread")]
async fn server_peers_sync_the_closure_and_nothing_else() {
    use automerge_repo::protocol::WireMessage;
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    a.add_user("bob").await;
    let b = peer_server(None).await;
    join(&b, &a.invite().await, "laptop-b").await.unwrap();
    let key_file = b.dir.path().join("server.key");
    let _b_dir = b.stop().await;

    // A member asks for a document an index lists: it gets it.
    let mut peer = RawPeer::connect(&key_file, a.peer_addr.unwrap()).await;
    let workspace = DocumentId::parse_any(&alice.workspace_doc).unwrap();
    peer.send(WireMessage::Request {
        sender_id: "srv:raw".into(),
        target_id: "x".into(),
        document_id: workspace,
        data: sync_data(),
    })
    .await;
    loop {
        match peer.next().await.expect("link stays open") {
            WireMessage::Sync { document_id, .. } if document_id == workspace => break,
            _ => {}
        }
    }
    // A member pushing a document reachable from nothing: discarded, and
    // the link closes with a protocol error.
    let stray = DocumentId::new();
    peer.send(WireMessage::Sync {
        sender_id: "srv:raw".into(),
        target_id: "x".into(),
        document_id: stray,
        data: sync_data(),
    })
    .await;
    let mut error = None;
    while let Some(message) = peer.next().await {
        if let WireMessage::Error { message, .. } = message {
            error = Some(message);
        }
    }
    let error = error.expect("a protocol error before the close");
    assert!(error.contains("not reachable from the registry"), "{error}");
    assert!(a.server().repo().open_document(stray).await.is_err());

    // A client sending a server-formatted sender id is still its user.
    let login = a.login("alice").await;
    let sender = format!("srv:{}", a.server_id());
    let client = Client::connect(&a, &login, &sender).await;
    client.find_ready(&alice.workspace_doc).await;
    let bob = admin::list_users(&a.db()).await.unwrap();
    let bob = bob.iter().find(|user| user.name == "bob").unwrap();
    let handle = client
        .repo
        .find(DocumentId::parse_any(&bob.workspace_doc).unwrap())
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_secs(10), handle.ready())
            .await
            .unwrap()
            .is_err(),
        "only its own user's documents"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_evicted_document_reaches_a_newly_linked_member() {
    let dir = tempfile::tempdir().unwrap();
    admin::init(&dir.path().join("server.db"), Some("laptop-a"))
        .await
        .unwrap();
    let mut a = TestServer::reopen(dir, |options| {
        options.idle_eviction = Duration::from_millis(200);
    })
    .await;
    a.listen_peers().await;
    let alice = a.add_user("alice").await;
    let workspace = DocumentId::parse_any(&alice.workspace_doc).unwrap();
    eventually("a evicts the workspace", 10, || async {
        a.server().repo().get(workspace).await.unwrap().is_none()
    })
    .await;
    let b = peer_server(None).await;
    join(&b, &a.invite().await, "laptop-b").await.unwrap();
    assert!(has_doc(&b, &alice.workspace_doc).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_join_killed_mid_pull_resumes_the_same_root_on_restart() {
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    // The proxy lets the pairing connection through, then nothing: the
    // joiner records its intent but cannot fetch.
    let proxy = Proxy::start(a.peer_addr.unwrap(), 1).await;
    let code = invite_at(&a, proxy.addr).await;
    let b = peer_server(None).await;
    let db = b.db();
    let joining = tokio::spawn(async move { admin::join(&db, &code, Some("laptop-b")).await });
    eventually("b is joining", 10, || async {
        b.app().state() == RootState::Joining
    })
    .await;
    // Clients are refused meanwhile.
    let (status, body) = common::post(
        &b.base(),
        "/api/login",
        None,
        serde_json::json!({"username": "alice", "password": common::PASSWORD}),
    )
    .await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["state"], "Joining");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(b.app().state(), RootState::Joining);

    // "Kill" B, then restart it with the source reachable.
    joining.abort();
    let dir = b.stop().await;
    let intent = Db::open(&dir.path().join("server.db"))
        .unwrap()
        .joining()
        .unwrap();
    assert_eq!(
        intent.unwrap().registry_doc,
        a.app().root().unwrap().registry_doc
    );
    proxy.open();
    let b = TestServer::reopen(dir, |_| {}).await;
    timeout(
        Duration::from_secs(20),
        b.app().wait_ready(Duration::from_secs(20)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        b.app().root().unwrap().registry_doc,
        a.app().root().unwrap().registry_doc
    );
    assert!(has_doc(&b, &alice.workspace_doc).await);
    b.login("alice").await;
    drop(proxy);
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_keeps_the_key_unless_asked_and_completes_after_a_crash() {
    let server = TestServer::start(|_| {}).await;
    server.add_user("alice").await;
    let id = server.server_id();
    let error = admin::reset(&server.db(), false)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("stop it"), "{error}");
    let dir = server.stop().await;
    let db = dir.path().join("server.db");

    admin::reset(&db, false).await.unwrap();
    assert_eq!(admin::server_id(&db).await.unwrap(), id, "key kept");
    let error = admin::list_users(&db).await.unwrap_err().to_string();
    assert!(error.contains("not set up"), "{error}");

    // A reset interrupted after deleting the documents completes on the
    // next start, here with a new identity.
    admin::init(&db, Some("again")).await.unwrap();
    admin::add_user(&db, "bob", common::PASSWORD).await.unwrap();
    {
        let database = Db::open(&db).unwrap();
        database.begin_reset(true).unwrap();
        database.reset_documents().unwrap();
    }
    let server = TestServer::reopen(dir, |_| {}).await;
    assert_eq!(server.app().state(), RootState::NeedsDecision);
    assert_ne!(server.server_id(), id, "new identity");
    assert!(
        Db::open(&server.db())
            .unwrap()
            .reset_intent()
            .unwrap()
            .is_none()
    );
    assert!(Db::open(&server.db()).unwrap().root().unwrap().is_none());
    let tokens = admin::list_tokens(&server.db()).await.unwrap();
    assert!(tokens.is_empty());
}

async fn cli(db: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_tt-server"))
        .args(args)
        .env("TT_SERVER_DB", db)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// `peer invite --listen` without a running server: prints the code, then
/// serves the pairing and exits once it is done.
async fn cli_invite(db: &std::path::Path) -> (String, tokio::process::Child) {
    use tokio::io::AsyncBufReadExt;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tt-server"))
        .args(["peer", "invite", "--listen", "127.0.0.1:0"])
        .env("TT_SERVER_DB", db)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let code = timeout(Duration::from_secs(20), lines.next_line())
        .await
        .expect("the code is printed")
        .unwrap()
        .expect("a line");
    (code, child)
}

#[tokio::test(flavor = "multi_thread")]
async fn peer_commands_without_a_running_server() {
    let dirs: Vec<tempfile::TempDir> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let [a, b, c] = [0, 1, 2].map(|i| dirs[i].path().join("server.db"));
    let (code, out, err) = cli(&a, &["init", "--name", "laptop-a"]).await;
    assert_eq!(code, 0, "{out}{err}");
    let (code, _, err) = cli(&a, &["peer", "invite"]).await;
    assert_eq!(code, 2, "no listener and no --listen: {err}");
    assert!(err.contains("--listen"), "{err}");

    // Two hosts both called nixos join A.
    for joiner in [&b, &c] {
        let (invite, child) = cli_invite(&a).await;
        let (code, out, err) = cli(joiner, &["peer", "join", &invite, "--name", "nixos"]).await;
        assert_eq!(code, 0, "{out}{err}");
        assert!(out.contains("joined laptop-a"), "{out}");
        let output = timeout(Duration::from_secs(20), child.wait_with_output())
            .await
            .expect("the inviting command exits once paired")
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let (code, out, _) = cli(&a, &["peer", "ls"]).await;
    assert_eq!(code, 0);
    let nixos: Vec<&str> = out
        .lines()
        .filter(|line| line.starts_with("nixos ("))
        .collect();
    assert_eq!(nixos.len(), 2, "{out}");
    assert_ne!(
        nixos[0][..16],
        nixos[1][..16],
        "distinct id prefixes: {out}"
    );
    assert!(out.contains("this server"), "{out}");
    let (code, _, err) = cli(&a, &["peer", "revoke", "nixos"]).await;
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("id prefix"), "{err}");
    let (code, own, _) = cli(&b, &["peer", "ls"]).await;
    assert_eq!(code, 0);
    let own_line = own
        .lines()
        .find(|line| line.ends_with("this server"))
        .unwrap();
    let own_prefix = own_line.split(['(', ')']).nth(1).unwrap().to_owned();
    let (code, _, err) = cli(&b, &["peer", "revoke", &own_prefix]).await;
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("tt-server reset"), "{err}");
    let (code, out, err) = cli(&a, &["peer", "revoke", &own_prefix]).await;
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("revoked nixos"), "{out}");
    let (code, out, err) = cli(&a, &["peer", "rename", "laptop-a", "desk"]).await;
    assert_eq!(code, 0, "{out}{err}");

    // Reset asks for confirmation; without a terminal it needs --yes.
    let (code, _, err) = cli(&c, &["reset"]).await;
    assert_ne!(code, 0);
    assert!(err.contains("--yes"), "{err}");
    let (code, out, err) = cli(&c, &["reset", "--yes"]).await;
    assert_eq!(code, 0, "{out}{err}");
    let (code, _, _) = cli(&c, &["user", "ls"]).await;
    assert_eq!(code, 3, "back in NeedsDecision");
}

#[tokio::test(flavor = "multi_thread")]
async fn init_records_this_server_as_the_first_member() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    let root = admin::init(&db, Some("laptop-a")).await.unwrap();
    let members = admin::list_servers(&db).await.unwrap();
    assert_eq!(members.len(), 1);
    let me = &members[0];
    assert_eq!(
        (me.id.as_str(), me.name.as_str()),
        (root.server_id.as_str(), "laptop-a")
    );
    let identity =
        tt_server::identity::Identity::load_or_create(&dir.path().join("server.key")).unwrap();
    assert_eq!(me.public_key().unwrap(), identity.public_key());
    assert!(!me.is_revoked());
}
