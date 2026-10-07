//! Lazy loading and idle eviction: a server-style repository loads stored
//! documents on first use, closes idle ones after flushing them, and reloads
//! them on demand without losing anything.
#![cfg(feature = "sqlite")]

use std::{sync::Arc, time::Duration};

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{
    DocHandle, DocumentId, DocumentStatus, Error, PeerId, Repo, RepoConfig, SqliteStorage,
    testing::{MemoryNetwork, MemoryTransport},
};
use tokio::time::{sleep, timeout};

fn lazy(idle: Option<Duration>) -> RepoConfig {
    RepoConfig {
        lazy_load: true,
        idle_eviction: idle,
        ..RepoConfig::default()
    }
}

async fn open(path: &std::path::Path, config: RepoConfig) -> Repo {
    let store = Arc::new(SqliteStorage::open(path).unwrap());
    let (transport, _other) = MemoryTransport::pair("solo", "nobody", 16);
    Repo::open(store.clone(), store, transport, config)
        .await
        .unwrap()
}

async fn value(handle: &DocHandle) -> Option<String> {
    handle
        .read(|doc| {
            doc.get(ROOT, "value")
                .unwrap()
                .and_then(|(value, _)| value.into_string().ok())
        })
        .await
        .unwrap()
}

async fn set(handle: &DocHandle, value: &'static str) {
    handle
        .change(move |tx| {
            tx.put(ROOT, "value", value)
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();
}

async fn create(repo: &Repo, value: &'static str) -> DocumentId {
    let handle = repo
        .create_with(move |tx| {
            tx.put(ROOT, "value", value)
                .map_err(|error| Error::Change(error.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();
    repo.flush().await.unwrap();
    handle.id()
}

#[tokio::test]
async fn lazy_open_lists_without_loading_and_loads_on_find() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.db");
    let repo = open(&path, RepoConfig::default()).await;
    let id = create(&repo, "stored").await;
    repo.shutdown().await.unwrap();

    let repo = open(&path, lazy(None)).await;
    assert_eq!(repo.document_ids().await.unwrap(), vec![id]);
    assert!(repo.get(id).await.unwrap().is_none(), "not loaded yet");
    let handle = repo.find(id).await.unwrap();
    assert_eq!(handle.status(), DocumentStatus::Ready);
    assert_eq!(value(&handle).await.as_deref(), Some("stored"));
    assert!(repo.get(id).await.unwrap().is_some());
    repo.shutdown().await.unwrap();
}

#[tokio::test]
async fn idle_documents_are_flushed_evicted_and_reloaded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.db");
    let repo = open(&path, lazy(Some(Duration::from_millis(50)))).await;
    let id = create(&repo, "first").await;
    let held = repo.find(id).await.unwrap();
    set(&held, "second").await;

    // A handle held outside the repository keeps the document loaded.
    sleep(Duration::from_millis(250)).await;
    assert!(repo.get(id).await.unwrap().is_some(), "held handle evicted");
    drop(held);

    timeout(Duration::from_secs(5), async {
        while repo.get(id).await.unwrap().is_some() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("idle document evicted");
    assert_eq!(repo.document_ids().await.unwrap(), vec![id]);

    // The change made just before eviction was flushed and survives a reload
    // and a restart.
    let handle = repo.find(id).await.unwrap();
    assert_eq!(value(&handle).await.as_deref(), Some("second"));
    drop(handle);
    repo.shutdown().await.unwrap();
    let repo = open(&path, lazy(None)).await;
    let handle = repo.find(id).await.unwrap();
    assert_eq!(value(&handle).await.as_deref(), Some("second"));
    repo.shutdown().await.unwrap();
}

async fn pump(network: &MemoryNetwork) {
    for _ in 0..50 {
        if network.deliver_all().await == 0 {
            sleep(Duration::from_millis(5)).await;
            if network.deliver_all().await == 0 {
                return;
            }
        }
    }
}

#[tokio::test]
async fn peers_get_lazily_loaded_documents_and_keep_them_resident() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.db");
    let seed = open(&path, RepoConfig::default()).await;
    let id = create(&seed, "on disk").await;
    seed.shutdown().await.unwrap();

    let network = MemoryNetwork::default();
    let server_id = PeerId::from("server");
    let client_id = PeerId::from("client");
    let store = Arc::new(SqliteStorage::open(&path).unwrap());
    let server = Repo::open(
        store.clone(),
        store,
        network.endpoint(server_id.clone(), 64),
        lazy(Some(Duration::from_millis(50))),
    )
    .await
    .unwrap();
    let client = Repo::open(
        Arc::new(automerge_repo::testing::MemoryStore::default()),
        Arc::new(automerge_repo::testing::MemoryStore::default()),
        network.endpoint(client_id.clone(), 64),
        RepoConfig::default(),
    )
    .await
    .unwrap();
    network.connect(&server_id, &client_id).await;
    let mut peers = client.subscribe_peers();
    peers.wait_for(|peers| !peers.is_empty()).await.unwrap();

    let handle = client.find(id).await.unwrap();
    timeout(Duration::from_secs(5), async {
        while handle.status() != DocumentStatus::Ready {
            pump(&network).await;
        }
    })
    .await
    .expect("client receives the lazily loaded document");
    assert_eq!(value(&handle).await.as_deref(), Some("on disk"));

    // Attached to a connected peer: never evicted.
    sleep(Duration::from_millis(250)).await;
    assert!(server.get(id).await.unwrap().is_some());

    network.disconnect(&server_id, &client_id).await;
    timeout(Duration::from_secs(5), async {
        while server.get(id).await.unwrap().is_some() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("evicted once the peer is gone");
}
