//! Peer addresses: the hello exchange and the local address store, the
//! registry left alone by roaming, hints only for members, the dialer and
//! revocation, one link per pair, and the status surfaces (`peer ls`,
//! `/api/peers`).

mod common;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{DocHandle, DocumentId, Error};
use common::{Client, Proxy, TestServer, eventually, get};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::tungstenite::Message;
use tt_server::{
    ServerOptions, admin,
    db::{AddressSource, Db},
    links::{Hello, Hint, HintAddr},
};

/// A server whose peer listener is bound before it starts, so that it can
/// advertise it (loopback addresses are not advertised on their own).
/// `advertise` defaults to the listener itself.
async fn member_in(
    dir: tempfile::TempDir,
    advertise: Option<Vec<String>>,
    configure: impl FnOnce(&mut ServerOptions),
) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let advertise = advertise.unwrap_or_else(|| vec![addr.to_string()]);
    let mut server = TestServer::reopen(dir, move |options| {
        options.peer_advertise = advertise;
        configure(options);
    })
    .await;
    server
        .server
        .as_mut()
        .unwrap()
        .spawn_peer_listener(listener);
    server.peer_addr = Some(addr);
    server
}

async fn member(
    init: Option<&str>,
    advertise: Option<Vec<String>>,
    configure: impl FnOnce(&mut ServerOptions),
) -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    if let Some(name) = init {
        admin::init(&dir.path().join("server.db"), Some(name))
            .await
            .unwrap();
    }
    member_in(dir, advertise, configure).await
}

/// A bound listener and a cuttable proxy in front of it.
async fn proxied() -> (TcpListener, Proxy) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = Proxy::start(listener.local_addr().unwrap(), -1).await;
    (listener, proxy)
}

/// Like [`member`], with the listener given (behind a proxy).
async fn member_behind(
    init: Option<&str>,
    listener: TcpListener,
    proxy: &Proxy,
    configure: impl FnOnce(&mut ServerOptions),
) -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    if let Some(name) = init {
        admin::init(&dir.path().join("server.db"), Some(name))
            .await
            .unwrap();
    }
    let advertise = vec![proxy.addr.to_string()];
    let addr = listener.local_addr().unwrap();
    let mut server = TestServer::reopen(dir, move |options| {
        options.peer_advertise = advertise;
        configure(options);
    })
    .await;
    server
        .server
        .as_mut()
        .unwrap()
        .spawn_peer_listener(listener);
    server.peer_addr = Some(addr);
    server
}

async fn join(server: &TestServer, inviter: &TestServer, name: &str) {
    admin::join(&server.db(), &inviter.invite().await, Some(name))
        .await
        .unwrap();
}

/// What `server` stores about `other`'s addresses.
fn addresses(server: &TestServer, other: &str) -> Vec<(String, AddressSource)> {
    Db::open(&server.db())
        .unwrap()
        .member_addresses(other)
        .unwrap()
        .into_iter()
        .map(|address| (address.addr, address.source))
        .collect()
}

fn advertised(server: &TestServer, other: &str, addr: &str) -> bool {
    addresses(server, other).contains(&(addr.to_owned(), AddressSource::Advertised))
}

async fn registry_heads(server: &TestServer) -> Vec<automerge::ChangeHash> {
    let id = DocumentId::parse_any(&server.app().root().unwrap().registry_doc).unwrap();
    let handle = server.server().repo().open_document(id).await.unwrap();
    let mut heads = handle.read(|doc| doc.get_heads()).await.unwrap();
    heads.sort();
    heads
}

async fn put(handle: &DocHandle, key: &'static str, value: &'static str) {
    handle
        .change(move |tx| {
            tx.put(ROOT, key, value)
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();
}

async fn value(handle: &DocHandle, key: &'static str) -> Option<String> {
    handle
        .read(move |doc| {
            doc.get(ROOT, key)
                .unwrap()
                .and_then(|(value, _)| value.into_string().ok())
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn members_store_each_others_advertised_addresses_and_client_urls() {
    let a = member(Some("laptop-a"), None, |options| {
        options.public_url = Some("https://laptop-a.example.ts.net".into());
    })
    .await;
    // B advertises a MagicDNS name.
    let b = member(
        None,
        Some(vec!["laptop-b.example.ts.net:8772".into()]),
        |options| options.public_url = Some("https://laptop-b.example.ts.net".into()),
    )
    .await;
    join(&b, &a, "laptop-b").await;
    a.wait_linked(&b, 10).await;
    b.wait_linked(&a, 10).await;
    let (a_id, b_id) = (a.server_id(), b.server_id());
    let a_addr = a.peer_addr.unwrap().to_string();
    eventually("each stores the other's addresses", 10, || async {
        advertised(&a, &b_id, "laptop-b.example.ts.net:8772") && advertised(&b, &a_id, &a_addr)
    })
    .await;
    let seen = Db::open(&a.db()).unwrap().peer_seen().unwrap();
    assert_eq!(
        seen[&b_id].public_url.as_deref(),
        Some("https://laptop-b.example.ts.net")
    );
    let seen = Db::open(&b.db()).unwrap().peer_seen().unwrap();
    assert_eq!(
        seen[&a_id].public_url.as_deref(),
        Some("https://laptop-a.example.ts.net")
    );

    // `/api/peers` reports the member with its client URL.
    a.add_user("alice").await;
    let login = a.login("alice").await;
    let (status, body) = get(&a.base(), "/api/peers", Some(&login.token)).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["server"]["id"], a_id);
    assert_eq!(body["server"]["name"], "laptop-a");
    let peers = body["peers"].as_array().unwrap();
    assert_eq!(peers.len(), 1, "{body}");
    let peer = &peers[0];
    assert_eq!(peer["id"], b_id);
    assert_eq!(peer["name"], "laptop-b");
    assert!(
        peer["state"] == "online" || peer["state"] == "syncing",
        "{peer}"
    );
    assert!(peer["address"].is_string(), "{peer}");
    assert!(peer["last_seen"].is_i64(), "{peer}");
    assert_eq!(peer["public_url"], "https://laptop-b.example.ts.net");
    assert_eq!(peer["error"], serde_json::Value::Null);
    let (status, _) = get(&a.base(), "/api/peers", None).await;
    assert_eq!(status, 401);
    let (status, _) = get(&a.base(), "/api/peers", Some("not-a-token")).await;
    assert_eq!(status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unpaired_server_lists_no_peers() {
    let server = TestServer::start(|_| {}).await;
    server.add_user("alice").await;
    let login = server.login("alice").await;
    let (status, body) = get(&server.base(), "/api/peers", Some(&login.token)).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["peers"], serde_json::json!([]));
    assert_eq!(body["server"]["name"], "test-server");
}

/// Connects with a member's key, sends `hello`, and reads the reply hello.
async fn raw_hello(key_file: &std::path::Path, addr: SocketAddr, hello: &Hello) -> Hello {
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
    socket
        .send(Message::Binary(hello.encode().unwrap().into()))
        .await
        .unwrap();
    let reply = loop {
        match timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("a hello within 10 s")
        {
            Some(Ok(Message::Binary(bytes))) => break Hello::decode(&bytes).unwrap(),
            Some(Ok(_)) => {}
            other => panic!("expected a hello, got {other:?}"),
        }
    };
    let _ = socket.close(None).await;
    reply
}

fn hint(server_id: &str, addrs: impl IntoIterator<Item = String>) -> Hint {
    Hint {
        server_id: server_id.to_owned(),
        addrs: addrs
            .into_iter()
            .map(|addr| HintAddr { addr, seen_at: 0 })
            .collect(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hints_are_kept_only_for_listed_members_and_capped() {
    let a = member(Some("laptop-a"), None, |_| {}).await;
    let b = member(None, None, |_| {}).await;
    let c = member(None, None, |_| {}).await;
    let d = member(None, None, |_| {}).await;
    join(&b, &a, "laptop-b").await;
    join(&c, &a, "laptop-c").await;
    join(&d, &a, "laptop-d").await;
    let (a_id, b_id, c_id, d_id) = (a.server_id(), b.server_id(), c.server_id(), d.server_id());
    admin::revoke_server(&a.db(), &c_id[..8]).await.unwrap();
    let key_file = b.dir.path().join("server.key");
    let _b = b.stop().await;
    let unknown = tt_server::identity::server_id(&[9; 32]);

    // B (stopped; its key used directly) passes hints on to A.
    let mut hello = Hello::new(&b_id, "laptop-b");
    hello.advertise = vec!["127.0.0.1:1".into()];
    hello.hints = vec![
        hint(&unknown, ["127.0.0.1:2".to_owned()]),
        hint(&c_id, ["127.0.0.1:3".to_owned()]),
        hint(&a_id, ["127.0.0.1:4".to_owned()]),
        hint(&b_id, ["127.0.0.1:5".to_owned()]),
        hint(&d_id, (10..50).map(|port| format!("127.0.0.1:{port}"))),
    ];
    let reply = raw_hello(&key_file, a.peer_addr.unwrap(), &hello).await;
    assert_eq!(reply.server_id, a_id);
    assert_eq!(reply.name, "laptop-a");

    eventually("a stores the hints for d", 10, || async {
        addresses(&a, &d_id)
            .iter()
            .filter(|(_, source)| *source == AddressSource::Hint)
            .count()
            == 16
    })
    .await;
    assert!(advertised(&a, &b_id, "127.0.0.1:1"));
    assert!(
        !addresses(&a, &b_id)
            .iter()
            .any(|(addr, _)| addr == "127.0.0.1:5"),
        "a member's own addresses come from its advertisement only"
    );
    assert!(addresses(&a, &unknown).is_empty(), "unknown id discarded");
    assert!(
        !addresses(&a, &c_id)
            .iter()
            .any(|(addr, _)| addr == "127.0.0.1:3"),
        "revoked id discarded"
    );
    assert!(
        addresses(&a, &a_id).is_empty(),
        "hints about itself discarded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn roaming_never_changes_the_registry() {
    let a = member(Some("laptop-a"), None, |_| {}).await;
    let mut b = member(None, None, |_| {}).await;
    join(&b, &a, "laptop-b").await;
    a.wait_linked(&b, 10).await;
    let b_id = b.server_id();
    // Let the pairing's registry changes settle on both sides.
    eventually("the registries agree", 10, || async {
        registry_heads(&a).await == registry_heads(&b).await
    })
    .await;
    let before = registry_heads(&a).await;

    for network in 0..4 {
        let dir = b.stop().await;
        let name = format!("laptop-b-{network}.example.ts.net:8772");
        // B links again from another network, under another name.
        b = member_in(dir, Some(vec![name.clone()]), |_| {}).await;
        a.wait_linked(&b, 15).await;
        eventually("a records b's new name", 10, || async {
            advertised(&a, &b_id, &name)
        })
        .await;
        if network > 0 {
            let previous = format!("laptop-b-{}.example.ts.net:8772", network - 1);
            assert!(
                addresses(&a, &b_id).contains(&(previous, AddressSource::Hint)),
                "a name no longer advertised becomes a hint"
            );
        }
    }
    assert_eq!(
        registry_heads(&a).await,
        before,
        "a's registry is unchanged"
    );
    assert_eq!(
        registry_heads(&b).await,
        before,
        "b's registry is unchanged"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_member_is_no_longer_dialed() {
    let (listener_a, proxy_a) = proxied().await;
    let a = member_behind(Some("laptop-a"), listener_a, &proxy_a, |_| {}).await;
    let (listener, proxy) = proxied().await;
    let b = member_behind(None, listener, &proxy, |_| {}).await;
    join(&b, &a, "laptop-b").await;
    let b_id = b.server_id();
    a.wait_linked(&b, 10).await;
    eventually("a knows b's address", 10, || async {
        advertised(&a, &b_id, &proxy.addr.to_string())
    })
    .await;

    // Cut the link, and keep B from reaching A: A dials B again.
    proxy_a.cut();
    proxy.cut();
    let mut links = a.app().subscribe_links();
    timeout(
        Duration::from_secs(10),
        links.wait_for(|linked| !linked.contains(&b_id)),
    )
    .await
    .unwrap()
    .unwrap();
    let cut = proxy.accepted();
    proxy.open();
    eventually("a redials b", 15, || async { proxy.accepted() > cut }).await;
    a.wait_linked(&b, 10).await;

    admin::revoke_server(&a.db(), &b_id[..8]).await.unwrap();
    timeout(
        Duration::from_secs(5),
        links.wait_for(|linked| !linked.contains(&b_id)),
    )
    .await
    .expect("the link closes within 5 s")
    .unwrap();
    // Let the dialer notice, then count: no more dials towards B.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = proxy.accepted();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(proxy.accepted(), after, "a stopped dialing b");
    assert!(!a.app().linked_servers().contains(&b_id));
    let peers = a.app().peer_status().await.unwrap();
    assert!(peers.iter().all(|peer| peer.id != b_id), "{peers:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn simultaneous_dials_leave_one_link_and_documents_converge() {
    let (listener_a, proxy_a) = proxied().await;
    let (listener_b, proxy_b) = proxied().await;
    let a = member_behind(Some("laptop-a"), listener_a, &proxy_a, |_| {}).await;
    let alice = a.add_user("alice").await;
    let b = member_behind(None, listener_b, &proxy_b, |_| {}).await;
    join(&b, &a, "laptop-b").await;
    let (a_id, b_id) = (a.server_id(), b.server_id());
    a.wait_linked(&b, 10).await;
    eventually("both know each other's address", 10, || async {
        advertised(&a, &b_id, &proxy_b.addr.to_string()) && !addresses(&b, &a_id).is_empty()
    })
    .await;
    let on_a = Client::connect(&a, &a.login("alice").await, "phone-a").await;
    let on_b = Client::connect(&b, &b.login("alice").await, "phone-b").await;
    let workspace_a = on_a.find_ready(&alice.workspace_doc).await;
    let workspace_b = on_b.find_ready(&alice.workspace_doc).await;

    for round in 0..3 {
        // Cut both ways, edit on both sides, and let both dial at once.
        proxy_a.cut();
        proxy_b.cut();
        eventually("both are unlinked", 10, || async {
            a.app().linked_servers().is_empty() && b.app().linked_servers().is_empty()
        })
        .await;
        let (key_a, key_b): (&'static str, &'static str) = match round {
            0 => ("a0", "b0"),
            1 => ("a1", "b1"),
            _ => ("a2", "b2"),
        };
        put(&workspace_a, key_a, "from a").await;
        put(&workspace_b, key_b, "from b").await;
        proxy_a.open();
        proxy_b.open();
        a.wait_linked(&b, 15).await;
        b.wait_linked(&a, 15).await;
        // Exactly one link survives, the same on both sides: when both
        // dials got through, the one the lower id dialed (which runs through
        // the proxy in front of the higher id's listener).
        let (preferred, other) = if a_id < b_id {
            (&proxy_b, &proxy_a)
        } else {
            (&proxy_a, &proxy_b)
        };
        eventually("one link remains", 15, || async {
            preferred.open_connections() + other.open_connections() == 1
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(preferred.open_connections() + other.open_connections(), 1);
        assert_eq!(a.app().linked_servers(), std::slice::from_ref(&b_id));
        assert_eq!(b.app().linked_servers(), std::slice::from_ref(&a_id));
        eventually("documents converge", 15, || async {
            value(&workspace_a, key_b).await.as_deref() == Some("from b")
                && value(&workspace_b, key_a).await.as_deref() == Some("from a")
        })
        .await;
    }
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

#[tokio::test(flavor = "multi_thread")]
async fn peer_ls_shows_online_and_offline_members() {
    let a = member(Some("laptop-a"), None, |_| {}).await;
    let (code, out, err) = cli(&a.db(), &["peer", "ls"]).await;
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("not paired"), "{out}");
    assert!(out.contains("tt-server peer invite"), "{out}");

    let b = member(None, None, |_| {}).await;
    let c = member(None, None, |_| {}).await;
    join(&b, &a, "laptop-b").await;
    join(&c, &a, "laptop-c").await;
    a.wait_linked(&b, 10).await;
    a.wait_linked(&c, 10).await;
    let c_id = c.server_id();
    let _c = c.stop().await;
    let mut links = a.app().subscribe_links();
    timeout(
        Duration::from_secs(10),
        links.wait_for(|linked| !linked.contains(&c_id)),
    )
    .await
    .unwrap()
    .unwrap();

    let (code, out, err) = cli(&a.db(), &["peer", "ls"]).await;
    assert_eq!(code, 0, "{err}");
    let mut lines = out.lines();
    assert!(lines.next().unwrap().starts_with("SERVER"), "{out}");
    let rows: Vec<&str> = lines.collect();
    assert_eq!(rows.len(), 2, "{out}");
    let b_row = rows
        .iter()
        .find(|row| row.starts_with("laptop-b ("))
        .unwrap();
    let c_row = rows
        .iter()
        .find(|row| row.starts_with("laptop-c ("))
        .unwrap();
    assert!(b_row.contains(" online "), "{out}");
    assert!(b_row.contains("127.0.0.1"), "the link address: {out}");
    assert!(c_row.contains(" offline "), "{out}");
    assert!(c_row.contains(" now "), "last seen just now: {out}");
    assert!(c_row.contains("127.0.0.1"), "the last link address: {out}");
    assert!(!out.contains("laptop-a"), "not this server: {out}");

    // `--all` lists every member, this server included.
    let (code, out, _) = cli(&a.db(), &["peer", "ls", "--all"]).await;
    assert_eq!(code, 0);
    assert!(out.contains("this server"), "{out}");
}
