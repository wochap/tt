use std::sync::Arc;

use automerge_repo::{
    DocumentId, PeerId,
    error::NetworkError,
    network::{NetworkEvent, NetworkTransport},
    storage::{ControlStore, StorageAdapter},
    testing::{MemoryStore, MemoryTransport},
};
use bytes::Bytes;

#[tokio::test]
async fn memory_storage_round_trips_and_injects_failures() {
    let store = MemoryStore::default();
    let id = DocumentId::new();
    assert_eq!(StorageAdapter::load(&store, id).await.unwrap(), None);
    StorageAdapter::store(&store, id, vec![1, 2, 3])
        .await
        .unwrap();
    assert_eq!(
        StorageAdapter::load(&store, id).await.unwrap(),
        Some(vec![1, 2, 3])
    );
    assert_eq!(ControlStore::get(&store, "storage-id").await.unwrap(), None);
    ControlStore::put(&store, "storage-id", b"abc".to_vec())
        .await
        .unwrap();
    assert_eq!(
        ControlStore::get(&store, "storage-id").await.unwrap(),
        Some(b"abc".to_vec())
    );
    assert_eq!(store.control_value("storage-id"), Some(b"abc".to_vec()));
    store.fail("remove");
    let error = StorageAdapter::remove(&store, id).await.unwrap_err();
    assert_eq!(error.document, Some(id));
    assert_eq!(error.operation, "remove");
}

#[tokio::test]
async fn memory_storage_blocking_is_explicitly_released() {
    let store = MemoryStore::default();
    store.block("store");
    let task_store = store.clone();
    let id = DocumentId::new();
    let pending =
        tokio::spawn(async move { StorageAdapter::store(&task_store, id, vec![7]).await });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished());
    store.unblock("store");
    pending.await.unwrap().unwrap();
}

#[tokio::test]
async fn authenticated_transport_is_manual_and_receiver_is_one_time() {
    let (a, b) = MemoryTransport::pair("a", "b", 8);
    assert_eq!(a.local_peer(), PeerId::from("a"));
    assert_eq!(b.local_peer(), PeerId::from("b"));
    let mut b_events = b.take_events().unwrap();
    assert_eq!(
        b.take_events().unwrap_err(),
        NetworkError::EventsAlreadyTaken
    );
    a.connect().await;
    assert_eq!(
        b_events.recv().await,
        Some(NetworkEvent::PeerConnected(PeerId::from("a")))
    );
    a.send(&PeerId::from("b"), Bytes::from_static(b"frame"))
        .await
        .unwrap();
    assert_eq!(a.pending_len(), 1);
    assert!(a.deliver_next().await);
    assert_eq!(
        b_events.recv().await,
        Some(NetworkEvent::Message {
            peer: PeerId::from("a"),
            bytes: Bytes::from_static(b"frame")
        })
    );
}

#[test]
fn adapters_are_cloneable_or_shareable() {
    let _: Arc<dyn StorageAdapter> = Arc::new(MemoryStore::default());
    let _: Arc<dyn ControlStore> = Arc::new(MemoryStore::default());
}
