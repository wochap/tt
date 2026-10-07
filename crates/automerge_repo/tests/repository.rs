use std::sync::Arc;

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{
    AccessPolicy, DocumentId, Error, PeerId, Repo, RepoConfig,
    document::{ChangeOrigin, DocumentStatus},
    error::ProtocolError,
    network::NetworkTransport,
    protocol::{PeerMetadata, WireMessage},
    testing::{MemoryNetwork, MemoryStore, MemoryTransport},
};
use bytes::Bytes;

fn put(
    tx: &mut automerge::transaction::Transaction<'_>,
    key: &str,
    value: i64,
) -> automerge_repo::Result<()> {
    tx.put(ROOT, key, value)
        .map_err(|error| Error::Change(error.to_string()))
}

async fn open_repo(transport: Arc<dyn NetworkTransport>) -> (Repo, MemoryStore, MemoryStore) {
    let docs = MemoryStore::default();
    let ctrl = MemoryStore::default();
    let repo = Repo::open(
        Arc::new(docs.clone()),
        Arc::new(ctrl.clone()),
        transport,
        RepoConfig::default(),
    )
    .await
    .unwrap();
    (repo, docs, ctrl)
}

async fn open_pair() -> (
    Repo,
    Repo,
    Arc<MemoryTransport>,
    Arc<MemoryTransport>,
    MemoryStore,
    MemoryStore,
) {
    let (net_a, net_b) = MemoryTransport::pair("a", "b", 256);
    let (a, docs_a, _) = open_repo(net_a.clone()).await;
    let (b, docs_b, _) = open_repo(net_b.clone()).await;
    (a, b, net_a, net_b, docs_a, docs_b)
}

async fn drive(a: &MemoryTransport, b: &MemoryTransport) {
    let mut idle = 0;
    for _ in 0..500 {
        tokio::task::yield_now().await;
        let delivered = a.deliver_all().await + b.deliver_all().await;
        if delivered == 0 {
            idle += 1;
        } else {
            idle = 0;
        }
        if idle >= 10 {
            return;
        }
    }
    panic!("deterministic network failed to become idle");
}

async fn state(handle: &automerge_repo::DocHandle) -> (Vec<automerge::ChangeHash>, String) {
    handle
        .read(|doc| {
            let mut keys: Vec<_> = doc.keys(ROOT).collect();
            keys.sort();
            let values: Vec<_> = keys
                .into_iter()
                .map(|key| {
                    let value = doc.get(ROOT, &key).unwrap();
                    (key, format!("{value:?}"))
                })
                .collect();
            (doc.get_heads(), format!("{values:?}"))
        })
        .await
        .unwrap()
}

/// Opens a connected pair where `a` has `doc` and `b` holds the synced copy.
async fn shared_document(
    a: &Repo,
    b: &Repo,
    net_a: &MemoryTransport,
    net_b: &MemoryTransport,
) -> (automerge_repo::DocHandle, automerge_repo::DocHandle) {
    net_a.connect().await;
    drive(net_a, net_b).await;
    let left = a.create().await.unwrap();
    drive(net_a, net_b).await;
    let right = b.find(left.id()).await.unwrap();
    right.ready().await.unwrap();
    (left, right)
}

#[tokio::test]
async fn creation_is_durable_and_ready_before_it_returns() {
    let (transport, _) = MemoryTransport::pair("repo", "peer", 8);
    let (repo, docs, control) = open_repo(transport).await;
    let document = repo.create().await.unwrap();
    assert_eq!(document.status(), DocumentStatus::Ready);
    let operations = docs.operations();
    let store = operations
        .iter()
        .position(|entry| entry == &format!("store:{}", document.id()))
        .expect("created document is stored");
    assert!(
        operations[store..]
            .iter()
            .any(|entry| entry == "document_flush"),
        "store is followed by a durability barrier: {operations:?}"
    );
    assert!(
        document
            .read(|doc| !doc.get_heads().is_empty())
            .await
            .unwrap()
    );
    assert!(control.operations().is_empty());
    assert_eq!(repo.document_ids().await.unwrap(), vec![document.id()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn created_documents_accept_changes_immediately_on_a_multi_thread_runtime() {
    let (transport, _) = MemoryTransport::pair("repo", "peer", 8);
    let (repo, _, _) = open_repo(transport).await;
    for index in 0..200 {
        let document = repo.create().await.unwrap();
        document
            .change(move |tx| put(tx, "index", index))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn document_change_rollback_noop_events_and_cached_handle() {
    let (repo, _, _, _, _, _) = open_pair().await;
    let document = repo.create().await.unwrap();
    let again = repo.open_document(document.id()).await.unwrap();
    assert_eq!(document.id(), again.id());
    assert_eq!(
        repo.get(document.id()).await.unwrap().unwrap().id(),
        document.id()
    );
    assert!(repo.get(DocumentId::new()).await.unwrap().is_none());
    let mut events = document.subscribe();
    let no_op = document.change(|_| Ok(7)).await.unwrap();
    assert!(no_op.hash.is_none());
    assert_eq!(no_op.value, 7);
    assert!(events.try_recv().is_err());
    let failed = document
        .change(|tx| {
            put(tx, "rolled_back", 1)?;
            Err::<(), _>(Error::Change("stop".into()))
        })
        .await;
    assert!(failed.is_err());
    assert!(events.try_recv().is_err());
    document.change(|tx| put(tx, "kept", 2)).await.unwrap();
    let event = events.recv().await.unwrap();
    assert_eq!(event.origin, ChangeOrigin::Local);
    assert!(!event.patches.is_empty());
    let rolled_back = document
        .read(|doc| doc.get(ROOT, "rolled_back").unwrap().is_none())
        .await
        .unwrap();
    assert!(rolled_back);
}

#[tokio::test]
async fn create_with_applies_initializer_atomically() {
    let (repo, _, _, _, _, _) = open_pair().await;
    let document = repo
        .create_with(|tx| {
            put(tx, "a", 1)?;
            put(tx, "b", 2)
        })
        .await
        .unwrap();
    let (_, values) = state(&document).await;
    assert!(values.contains("\"a\"") && values.contains("\"b\""));
    let failed = repo
        .create_with(|tx| {
            put(tx, "a", 1)?;
            Err(Error::Change("refused".into()))
        })
        .await;
    assert!(matches!(failed, Err(Error::Creation { .. })));
    assert_eq!(repo.document_ids().await.unwrap(), vec![document.id()]);
}

#[tokio::test]
async fn existing_documents_are_announced_on_connect_and_found_by_the_peer() {
    let (a, b, net_a, net_b, _, _) = open_pair().await;
    let doc_a = a.create_with(|tx| put(tx, "answer", 42)).await.unwrap();
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    let doc_b = b.find(doc_a.id()).await.unwrap();
    doc_b.ready().await.unwrap();
    assert_eq!(state(&doc_a).await, state(&doc_b).await);
    // A document created while connected is announced live.
    let live = a.create_with(|tx| put(tx, "live", 1)).await.unwrap();
    drive(&net_a, &net_b).await;
    let live_b = b.open_document(live.id()).await.unwrap();
    assert_eq!(state(&live).await, state(&live_b).await);
}

/// Serves documents only on explicit request, like a relay that must not
/// push every document to every client.
struct RequestOnly;
impl AccessPolicy for RequestOnly {
    fn may_sync(&self, _: &PeerId, _: DocumentId) -> bool {
        true
    }
    fn may_announce(&self, _: &PeerId, _: DocumentId) -> bool {
        false
    }
}

#[tokio::test]
async fn find_requests_a_document_the_peer_does_not_announce() {
    let (net_a, net_b) = MemoryTransport::pair("server", "client", 256);
    let server = Repo::open_with_policy(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryStore::default()),
        net_a.clone(),
        RepoConfig::default(),
        Arc::new(RequestOnly),
    )
    .await
    .unwrap();
    let (client, _, _) = open_repo(net_b.clone()).await;
    let doc = server.create_with(|tx| put(tx, "x", 1)).await.unwrap();
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    assert!(client.get(doc.id()).await.unwrap().is_none());
    let found = client.find(doc.id()).await.unwrap();
    assert_eq!(found.status(), DocumentStatus::Loading);
    drive(&net_a, &net_b).await;
    found.ready().await.unwrap();
    assert_eq!(state(&doc).await, state(&found).await);
    // Edits flow both ways once the client holds the document.
    found.change(|tx| put(tx, "y", 2)).await.unwrap();
    drive(&net_a, &net_b).await;
    assert_eq!(state(&doc).await, state(&found).await);
}

#[tokio::test]
async fn find_without_peers_is_unavailable_and_recovers_when_a_holder_connects() {
    let (a, b, net_a, net_b, _, _) = open_pair().await;
    let doc_a = a.create_with(|tx| put(tx, "late", 1)).await.unwrap();
    let pending = b.find(doc_a.id()).await.unwrap();
    assert_eq!(pending.status(), DocumentStatus::Unavailable);
    assert!(matches!(
        pending.ready().await,
        Err(Error::Lifecycle(
            automerge_repo::error::LifecycleError::DocumentUnavailable { .. }
        ))
    ));
    assert!(matches!(
        pending.change(|tx| put(tx, "x", 1)).await,
        Err(Error::Lifecycle(
            automerge_repo::error::LifecycleError::DocumentNotReady { .. }
        ))
    ));
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    pending.ready().await.unwrap();
    assert_eq!(state(&doc_a).await, state(&pending).await);
}

#[tokio::test]
async fn find_becomes_unavailable_when_every_peer_answers_doc_unavailable() {
    let (_, b, net_a, net_b, _, _) = open_pair().await;
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    let missing = b.find(DocumentId::new()).await.unwrap();
    assert_eq!(missing.status(), DocumentStatus::Loading);
    drive(&net_a, &net_b).await;
    assert_eq!(missing.status(), DocumentStatus::Unavailable);
    // A fresh session asks again.
    net_a.connect().await;
    tokio::task::yield_now().await;
    drive(&net_a, &net_b).await;
    assert_eq!(missing.status(), DocumentStatus::Unavailable);
}

#[tokio::test]
async fn find_of_a_stored_document_returns_it_without_network() {
    let (repo, _, _, _, _, _) = open_pair().await;
    let document = repo.create().await.unwrap();
    let found = repo.find(document.id()).await.unwrap();
    assert_eq!(found.status(), DocumentStatus::Ready);
    assert!(matches!(
        repo.open_document(DocumentId::new()).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn concurrent_offline_changes_converge_after_fresh_reconnection() {
    let (a, b, net_a, net_b, _, _) = open_pair().await;
    let (left, right) = shared_document(&a, &b, &net_a, &net_b).await;
    net_a.disconnect().await;
    tokio::task::yield_now().await;
    left.change(|tx| put(tx, "left", 1)).await.unwrap();
    right.change(|tx| put(tx, "right", 2)).await.unwrap();
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    assert_eq!(state(&left).await, state(&right).await);
}

#[tokio::test]
async fn shutdown_closes_handles_and_repository() {
    let (repo, _, _, _, _, _) = open_pair().await;
    let document = repo.create().await.unwrap();
    repo.clone().shutdown().await.unwrap();
    assert_eq!(document.status(), DocumentStatus::Closed);
    assert!(repo.document_ids().await.is_err());
    assert!(repo.create().await.is_err());
    assert!(document.read(|_| ()).await.is_err());
}

async fn raw_repo() -> (Repo, Arc<MemoryTransport>, Arc<MemoryTransport>) {
    let (net_repo, raw_peer) = MemoryTransport::pair("repo", "raw", 64);
    let (repo, _, _) = open_repo(net_repo.clone()).await;
    net_repo.connect().await;
    tokio::task::yield_now().await;
    (repo, net_repo, raw_peer)
}

async fn send_raw(raw: &MemoryTransport, message: &WireMessage) {
    raw.send(
        &PeerId::from("repo"),
        Bytes::from(message.encode().unwrap()),
    )
    .await
    .unwrap();
    raw.deliver_all().await;
    tokio::task::yield_now().await;
}

#[tokio::test]
async fn handshake_message_after_connect_is_a_peer_scoped_protocol_error() {
    let (repo, _, raw) = raw_repo().await;
    let mut errors = repo.subscribe_errors();
    let mut peers = repo.subscribe_peers();
    peers
        .wait_for(|peers| peers.contains(&PeerId::from("raw")))
        .await
        .unwrap();
    send_raw(
        &raw,
        &WireMessage::Join {
            sender_id: "raw".into(),
            peer_metadata: PeerMetadata::default(),
            supported_protocol_versions: vec!["1".into()],
        },
    )
    .await;
    assert!(matches!(
        errors.recv().await.unwrap(),
        Error::Protocol(ProtocolError::UnexpectedHandshake(kind)) if kind == "join"
    ));
    peers
        .wait_for(|peers| !peers.contains(&PeerId::from("raw")))
        .await
        .unwrap();
}

#[tokio::test]
async fn malformed_frames_and_sync_payloads_are_protocol_errors() {
    let (repo, net_repo, raw) = raw_repo().await;
    let mut errors = repo.subscribe_errors();
    raw.send(&PeerId::from("repo"), Bytes::from_static(&[0xff, 0x00]))
        .await
        .unwrap();
    raw.deliver_all().await;
    assert!(matches!(
        errors.recv().await.unwrap(),
        Error::Protocol(ProtocolError::Cbor(_))
    ));
    net_repo.connect().await;
    tokio::task::yield_now().await;
    send_raw(
        &raw,
        &WireMessage::Sync {
            sender_id: "raw".into(),
            target_id: "repo".into(),
            document_id: DocumentId::new(),
            data: vec![1, 2, 3],
        },
    )
    .await;
    assert!(matches!(
        errors.recv().await.unwrap(),
        Error::Protocol(ProtocolError::InvalidSyncPayload)
    ));
}

#[tokio::test]
async fn duplicate_announcements_are_idempotent() {
    let (repo, _, raw) = raw_repo().await;
    let mut source = automerge::Automerge::new();
    source
        .transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.put(ROOT, "k", 1)?;
            Ok(())
        })
        .unwrap();
    let unknown = DocumentId::new();
    for _ in 0..2 {
        let mut sync_state = automerge::sync::State::new();
        let message =
            automerge::sync::SyncDoc::generate_sync_message(&source, &mut sync_state).unwrap();
        send_raw(
            &raw,
            &WireMessage::Sync {
                sender_id: "raw".into(),
                target_id: "repo".into(),
                document_id: unknown,
                data: message.encode(),
            },
        )
        .await;
    }
    let ids = repo.document_ids().await.unwrap();
    assert_eq!(ids.iter().filter(|id| **id == unknown).count(), 1);
}

#[tokio::test]
async fn send_failure_is_reported_and_reconnection_uses_fresh_state() {
    let (a, b, net_a, net_b, _, _) = open_pair().await;
    let (left, right) = shared_document(&a, &b, &net_a, &net_b).await;
    let mut errors = a.subscribe_errors();
    net_a.fail_sends(true);
    left.change(|tx| put(tx, "after-failure", 1)).await.unwrap();
    tokio::task::yield_now().await;
    assert!(matches!(
        errors.recv().await.unwrap(),
        Error::Network(automerge_repo::error::NetworkError::Transport { .. })
    ));
    net_a.fail_sends(false);
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    assert_eq!(state(&left).await, state(&right).await);
}

async fn drive_hub(network: &MemoryNetwork) {
    let mut idle = 0;
    for _ in 0..1000 {
        tokio::task::yield_now().await;
        let delivered = network.deliver_all().await;
        if delivered == 0 {
            idle += 1;
        } else {
            idle = 0;
        }
        if idle >= 12 {
            return;
        }
    }
    panic!("network did not become idle");
}

#[tokio::test]
async fn three_peer_updates_are_relayed_and_multiple_documents_propagate() {
    let network = MemoryNetwork::default();
    let a_id = PeerId::from("a");
    let b_id = PeerId::from("b");
    let c_id = PeerId::from("c");
    let (a, _, _) = open_repo(network.endpoint(a_id.clone(), 512)).await;
    let (b, _, _) = open_repo(network.endpoint(b_id.clone(), 512)).await;
    let (c, _, _) = open_repo(network.endpoint(c_id.clone(), 512)).await;
    network.connect(&a_id, &b_id).await;
    network.connect(&b_id, &c_id).await;
    drive_hub(&network).await;
    let first = a.create_with(|tx| put(tx, "from-a", 1)).await.unwrap();
    let second = b.create_with(|tx| put(tx, "from-b", 2)).await.unwrap();
    drive_hub(&network).await;
    // `a` and `c` are not linked: `first` reaches `c` only through `b`.
    let first_c = c.open_document(first.id()).await.unwrap();
    let second_c = c.open_document(second.id()).await.unwrap();
    let second_a = a.open_document(second.id()).await.unwrap();
    assert_eq!(state(&first).await, state(&first_c).await);
    assert_eq!(state(&second).await, state(&second_c).await);
    assert_eq!(state(&second).await, state(&second_a).await);
    // Live edits are relayed too.
    first_c.change(|tx| put(tx, "from-c", 3)).await.unwrap();
    drive_hub(&network).await;
    assert_eq!(state(&first).await, state(&first_c).await);
}

#[tokio::test]
async fn deterministic_disconnect_schedule_eventually_converges() {
    let (a, b, net_a, net_b, _, _) = open_pair().await;
    let (left, right) = shared_document(&a, &b, &net_a, &net_b).await;
    let mut seed = 7_u64;
    for round in 0..24 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        if seed & 3 == 0 {
            net_a.disconnect().await;
        }
        let key = format!("k{round}");
        if seed & 1 == 0 {
            left.change(move |tx| put(tx, &key, round)).await.unwrap();
        } else {
            right.change(move |tx| put(tx, &key, round)).await.unwrap();
        }
        if seed & 3 == 1 {
            net_a.connect().await;
            drive(&net_a, &net_b).await;
        }
    }
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    assert_eq!(state(&left).await, state(&right).await);
}

#[tokio::test]
async fn relay_serves_requests_for_documents_it_is_still_persisting() {
    let network = MemoryNetwork::default();
    let a_id = PeerId::from("a");
    let relay_id = PeerId::from("relay");
    let b_id = PeerId::from("b");
    let (a, _, _) = open_repo(network.endpoint(a_id.clone(), 512)).await;
    let (_relay, relay_docs, _) = open_repo(network.endpoint(relay_id.clone(), 512)).await;
    let (b, _, _) = open_repo(network.endpoint(b_id.clone(), 512)).await;
    // The relay receives the content but cannot make it durable yet.
    relay_docs.block("store");
    let document = a.create_with(|tx| put(tx, "x", 1)).await.unwrap();
    network.connect(&a_id, &relay_id).await;
    drive_hub(&network).await;
    relay_docs
        .wait_for_operation(&format!("store:{}", document.id()))
        .await;

    network.connect(&b_id, &relay_id).await;
    let found = b.find(document.id()).await.unwrap();
    drive_hub(&network).await;
    tokio::time::timeout(std::time::Duration::from_secs(1), found.ready())
        .await
        .expect("document arrives")
        .expect("relay holding the content must not answer doc-unavailable");
    assert_eq!(state(&document).await, state(&found).await);
    relay_docs.unblock("store");
}
