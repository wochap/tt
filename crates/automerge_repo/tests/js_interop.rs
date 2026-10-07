//! Spike gate: Rust clients sync through the real automerge-repo JS sync
//! server, and a stock JS client syncs through the Rust server transport.

#[path = "support/node.rs"]
mod node;

use std::{sync::Arc, time::Duration};

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{
    ChangeOrigin, DocHandle, DocumentId, Error, PeerSyncState, Repo, RepoConfig,
    testing::MemoryStore,
    transport::{ConnectionState, WsJsClient, WsJsClientConfig, WsJsServer},
};
use tokio::time::timeout;

async fn client(url: &str, peer: &str) -> (Repo, Arc<WsJsClient>) {
    let transport = WsJsClient::start(WsJsClientConfig::new(url, peer));
    let repo = Repo::open(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryStore::default()),
        transport.clone(),
        RepoConfig::default(),
    )
    .await
    .unwrap();
    let mut state = transport.subscribe_state();
    timeout(
        Duration::from_secs(10),
        state.wait_for(|state| matches!(state, ConnectionState::Connected { .. })),
    )
    .await
    .expect("client connects within 10 s")
    .unwrap();
    // The transport reports Connected before the repository coordinator has
    // processed PeerConnected; `find` without a known peer is Unavailable.
    let mut peers = repo.subscribe_peers();
    timeout(
        Duration::from_secs(10),
        peers.wait_for(|peers| !peers.is_empty()),
    )
    .await
    .expect("repository sees the server within 10 s")
    .unwrap();
    (repo, transport)
}

/// Reads `title` whether it is a scalar string (Rust `put`) or a text object
/// (JS assignment).
async fn title(handle: &DocHandle) -> Option<String> {
    handle
        .read(|doc| match doc.get(ROOT, "title").unwrap()? {
            (automerge::Value::Object(automerge::ObjType::Text), id) => doc.text(id).ok(),
            (value, _) => value.into_string().ok(),
        })
        .await
        .unwrap()
}

async fn wait_synced(repo: &Repo, document: DocumentId) {
    let mut progress = repo.subscribe_peer_sync();
    timeout(
        Duration::from_secs(5),
        progress.wait_for(|peers| {
            peers.values().any(|peer| {
                peer.state == PeerSyncState::Synced
                    && peer.documents > 0
                    && !peer.syncing_documents.contains(&document)
            })
        }),
    )
    .await
    .expect("document reaches the server within 5 s")
    .unwrap();
}

async fn round_trip(url: &str) {
    let (a, _ta) = client(url, "rust-a").await;
    let doc_a = a
        .create_with(|tx| {
            tx.put(ROOT, "title", "from A")
                .map_err(|error| Error::Change(error.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();
    a.flush().await.unwrap();
    wait_synced(&a, doc_a.id()).await;

    let (b, _tb) = client(url, "rust-b").await;
    let doc_b = b.find(doc_a.id()).await.unwrap();
    timeout(Duration::from_secs(5), doc_b.ready())
        .await
        .expect("B receives the document within 5 s")
        .unwrap();
    assert_eq!(title(&doc_b).await.as_deref(), Some("from A"));

    let mut events = doc_a.subscribe();
    doc_b
        .change(|tx| {
            tx.put(ROOT, "title", "edited by B")
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event.origin, ChangeOrigin::Remote(_))
                && title(&doc_a).await.as_deref() == Some("edited by B")
            {
                return;
            }
        }
    })
    .await
    .expect("A observes B's change within 5 s");
}

#[tokio::test(flavor = "multi_thread")]
async fn rust_clients_round_trip_through_node_sync_server() {
    let Some(server) = node::SyncServer::start().await else {
        return;
    };
    round_trip(&server.url()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_document_is_reported_unavailable_by_node_server() {
    let Some(server) = node::SyncServer::start().await else {
        return;
    };
    let (b, _tb) = client(&server.url(), "rust-lonely").await;
    let handle = b.find(DocumentId::new()).await.unwrap();
    let result = timeout(Duration::from_secs(5), handle.ready())
        .await
        .expect("unavailability is reported within 5 s");
    assert!(result.is_err(), "missing document must not become ready");
}

#[tokio::test(flavor = "multi_thread")]
async fn rust_clients_round_trip_through_rust_server_transport() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let transport = WsJsServer::new("rust-server");
    let server = Repo::open(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryStore::default()),
        transport.clone(),
        RepoConfig::default(),
    )
    .await
    .unwrap();
    tokio::spawn(transport.listen(listener));
    round_trip(&format!("ws://127.0.0.1:{port}")).await;
    drop(server);
}

/// A stock `@automerge/automerge-repo` node client with the websocket client
/// adapter reads a document created by a Rust client through the Rust
/// server, then writes to it.
#[tokio::test(flavor = "multi_thread")]
async fn node_client_syncs_through_rust_server_transport() {
    let Some(dir) = node::install().await else {
        return;
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let transport = WsJsServer::new("rust-server");
    let _server = Repo::open(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryStore::default()),
        transport.clone(),
        RepoConfig::default(),
    )
    .await
    .unwrap();
    tokio::spawn(transport.listen(listener));
    let url = format!("ws://127.0.0.1:{port}");
    let (a, _ta) = client(&url, "rust-a").await;
    let doc = a
        .create_with(|tx| {
            tx.put(ROOT, "title", "from rust")
                .map_err(|error| Error::Change(error.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();
    wait_synced(&a, doc.id()).await;
    let script = r#"
        import { Repo } from "@automerge/automerge-repo";
        import { WebSocketClientAdapter } from "@automerge/automerge-repo-network-websocket";
        const [url, id] = process.argv.slice(2);
        const repo = new Repo({ network: [new WebSocketClientAdapter(url)], peerId: "node-client" });
        const handle = await repo.find(`automerge:${id}`);
        const doc = handle.doc();
        if (String(doc.title) !== "from rust") { console.error("bad title", doc.title); process.exit(2); }
        handle.change(d => { d.title = "from node"; });
        await new Promise(r => setTimeout(r, 1000));
        process.exit(0);
    "#;
    std::fs::write(dir.join("client.mjs"), script).unwrap();
    let mut events = doc.subscribe();
    let status = timeout(
        Duration::from_secs(20),
        tokio::process::Command::new("node")
            .arg("client.mjs")
            .arg(&url)
            .arg(doc.id().to_bs58check())
            .current_dir(&dir)
            .status(),
    )
    .await
    .expect("node client finishes within 20 s")
    .unwrap();
    assert!(status.success(), "node client failed: {status}");
    timeout(Duration::from_secs(5), async {
        loop {
            if title(&doc).await.as_deref() == Some("from node") {
                return;
            }
            let _ = events.recv().await;
        }
    })
    .await
    .expect("Rust client observes the node client's change");
}
