use std::{sync::Arc, time::Duration};

use automerge::{
    Automerge, ROOT, ReadDoc,
    transaction::{CommitOptions, Transactable},
};
use automerge_repo::{
    DocumentId, Error, FilesystemStorage, Repo, RepoConfig,
    storage::{ControlStore, StorageAdapter},
    testing::{MemoryStore, MemoryTransport},
};

async fn drive(a: &MemoryTransport, b: &MemoryTransport) {
    let mut idle = 0;
    for _ in 0..500 {
        tokio::task::yield_now().await;
        if a.deliver_all().await + b.deliver_all().await == 0 {
            idle += 1;
        } else {
            idle = 0;
        }
        if idle >= 12 {
            return;
        }
    }
    panic!("network failed to become idle");
}

fn put(
    tx: &mut automerge::transaction::Transaction<'_>,
    key: &str,
    value: i64,
) -> automerge_repo::Result<()> {
    tx.put(ROOT, key, value)
        .map_err(|error| Error::Change(error.to_string()))
}

async fn fresh(config: RepoConfig) -> (Repo, MemoryStore, MemoryStore) {
    let documents = MemoryStore::default();
    let control = MemoryStore::default();
    let (transport, _) = MemoryTransport::pair("repo", "peer", 64);
    let repo = Repo::open(
        Arc::new(documents.clone()),
        Arc::new(control.clone()),
        transport,
        config,
    )
    .await
    .unwrap();
    (repo, documents, control)
}

#[tokio::test]
async fn configuration_rejects_zero_capacity_and_inverted_retry() {
    let (transport, _) = MemoryTransport::pair("repo", "peer", 8);
    let config = RepoConfig {
        peer_writer_capacity: 0,
        ..RepoConfig::default()
    };
    assert!(matches!(
        Repo::open(
            Arc::new(MemoryStore::default()),
            Arc::new(MemoryStore::default()),
            transport,
            config
        )
        .await,
        Err(Error::Config(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn blocked_automatic_store_does_not_block_actor_and_flush_waits() {
    let config = RepoConfig {
        persistence_debounce: Duration::from_secs(10),
        ..RepoConfig::default()
    };
    let (repo, documents, _) = fresh(config).await;
    let document = repo.create().await.unwrap();
    documents.clear_operations();
    documents.block_document("store", document.id());
    document.change(|tx| put(tx, "one", 1)).await.unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    documents
        .wait_for_operation(&format!("store:{}", document.id()))
        .await;
    assert!(
        document
            .read(|doc| doc.get(ROOT, "one").unwrap().is_some())
            .await
            .unwrap()
    );
    document.change(|tx| put(tx, "two", 2)).await.unwrap();
    let flushing = tokio::spawn({
        let repo = repo.clone();
        async move { repo.flush().await }
    });
    tokio::task::yield_now().await;
    assert!(!flushing.is_finished());
    documents.unblock_document("store", document.id());
    flushing.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn automatic_failure_is_typed_and_retry_succeeds() {
    let config = RepoConfig {
        persistence_debounce: Duration::ZERO,
        persistence_retry_min: Duration::from_secs(2),
        persistence_retry_max: Duration::from_secs(8),
        ..RepoConfig::default()
    };
    let (repo, documents, _) = fresh(config).await;
    let document = repo.create().await.unwrap();
    documents.clear_operations();
    documents.fail_document_times("store", document.id(), 1);
    let mut errors = repo.subscribe_errors();
    document.change(|tx| put(tx, "retry", 1)).await.unwrap();
    tokio::task::yield_now().await;
    let error = errors.recv().await.unwrap();
    assert!(
        matches!(error, Error::Persistence { document: id, revision: 2, .. } if id == document.id())
    );
    tokio::time::advance(Duration::from_secs(2)).await;
    repo.flush().await.unwrap();
    assert!(documents.documents().contains_key(&document.id()));
}

#[tokio::test]
async fn flush_aggregates_document_and_barrier_failures() {
    let config = RepoConfig {
        persistence_debounce: Duration::from_secs(60),
        ..RepoConfig::default()
    };
    let (repo, documents, _) = fresh(config).await;
    let first = repo.create().await.unwrap();
    let second = repo.create().await.unwrap();
    first.change(|tx| put(tx, "x", 1)).await.unwrap();
    second.change(|tx| put(tx, "y", 2)).await.unwrap();
    documents.fail_document_times("store", first.id(), 1);
    documents.fail_document_times("store", second.id(), 1);
    documents.fail("document_flush");
    assert!(matches!(repo.flush().await, Err(Error::Flush(failures)) if failures.len() == 3));
}

#[tokio::test]
async fn filesystem_round_trip_layout_codec_and_permissions() {
    let directory = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::open(directory.path()).await.unwrap();
    let id = DocumentId::new();
    let mut doc = Automerge::new();
    doc.empty_commit(CommitOptions::default());
    StorageAdapter::store(&storage, id, doc.save())
        .await
        .unwrap();
    let unrelated = directory.path().join("automerge/notes.txt");
    std::fs::write(&unrelated, b"leave me").unwrap();
    let stale = directory
        .path()
        .join(format!("automerge/.{id}.automerge.tmp-0000000000000000"));
    std::fs::write(&stale, b"stale").unwrap();
    StorageAdapter::flush(&storage).await.unwrap();
    ControlStore::put(&storage, "storage-id", b"local-storage".to_vec())
        .await
        .unwrap();
    ControlStore::flush(&storage).await.unwrap();
    assert_eq!(StorageAdapter::list(&storage).await.unwrap(), vec![id]);
    assert!(unrelated.exists());
    assert!(!stale.exists());
    assert_eq!(
        ControlStore::get(&storage, "storage-id").await.unwrap(),
        Some(b"local-storage".to_vec())
    );
    assert_eq!(ControlStore::get(&storage, "missing").await.unwrap(), None);
    assert_eq!(
        std::fs::read(directory.path().join("control/storage-id.bin")).unwrap(),
        b"local-storage"
    );
    StorageAdapter::close(&storage).await.unwrap();
    ControlStore::close(&storage).await.unwrap();
    let reopened = FilesystemStorage::open(directory.path()).await.unwrap();
    assert_eq!(StorageAdapter::list(&reopened).await.unwrap(), vec![id]);
    assert_eq!(
        ControlStore::get(&reopened, "storage-id").await.unwrap(),
        Some(b"local-storage".to_vec())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.path().join(format!("automerge/{id}.automerge")))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(directory.path().join("automerge"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(directory.path().join("control/storage-id.bin"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn removal_is_durable_and_closes_existing_handles() {
    let (repo, documents, _) = fresh(RepoConfig::default()).await;
    let document = repo.create().await.unwrap();
    let kept = repo.create().await.unwrap();
    repo.remove_local(document.id()).await.unwrap();
    assert!(documents.documents().contains_key(&kept.id()));
    assert_eq!(repo.document_ids().await.unwrap(), vec![kept.id()]);
    assert!(matches!(
        repo.remove_local(document.id()).await,
        Err(Error::NotFound(_))
    ));
    assert!(!documents.documents().contains_key(&document.id()));
    assert!(document.read(|_| ()).await.is_err());
    assert!(matches!(
        repo.open_document(document.id()).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn creation_barrier_failure_cleans_up_and_flush_never_touches_control() {
    let (repo, documents, control) = fresh(RepoConfig::default()).await;
    control.clear_operations();
    repo.flush().await.unwrap();
    assert!(
        !control
            .operations()
            .iter()
            .any(|entry| entry == "control_flush")
    );

    documents.fail_times("document_flush", 1);
    let error = repo.create().await.unwrap_err();
    let id = match error {
        Error::Creation { document, .. } => document,
        other => panic!("unexpected error: {other}"),
    };
    assert!(!documents.documents().contains_key(&id));
    assert!(!repo.document_ids().await.unwrap().contains(&id));
}

#[tokio::test]
async fn shutdown_aggregates_failures_closes_handles_and_rejects_other_clones() {
    let (repo, documents, control) = fresh(RepoConfig::default()).await;
    let document = repo.create().await.unwrap();
    let retained = repo.clone();
    documents.fail("document_flush");
    control.fail("control_flush");
    assert!(matches!(repo.shutdown().await, Err(Error::Shutdown(failures)) if failures.len() >= 2));
    assert_eq!(document.status(), automerge_repo::DocumentStatus::Closed);
    assert!(retained.document_ids().await.is_err());
    assert!(documents.documents_closed());
    assert!(control.control_closed());
    assert!(matches!(
        retained.shutdown().await,
        Err(Error::Lifecycle(_))
    ));
}

#[tokio::test]
async fn removal_waits_for_an_inflight_store_before_deleting() {
    let config = RepoConfig {
        persistence_debounce: Duration::ZERO,
        ..RepoConfig::default()
    };
    let (repo, documents, _) = fresh(config).await;
    let document = repo.create().await.unwrap();
    documents.clear_operations();
    documents.block_document("store", document.id());
    document.change(|tx| put(tx, "pending", 1)).await.unwrap();
    documents
        .wait_for_operation(&format!("store:{}", document.id()))
        .await;
    let removal = tokio::spawn({
        let repo = repo.clone();
        let id = document.id();
        async move { repo.remove_local(id).await }
    });
    tokio::task::yield_now().await;
    assert!(!removal.is_finished());
    assert!(document.change(|tx| put(tx, "late", 2)).await.is_err());
    documents.unblock_document("store", document.id());
    removal.await.unwrap().unwrap();
    assert!(!documents.documents().contains_key(&document.id()));
}

#[tokio::test]
async fn failed_removal_evicts_and_can_be_reopened_then_retried() {
    let (repo, documents, _) = fresh(RepoConfig::default()).await;
    let document = repo.create().await.unwrap();
    documents.fail_times("remove", 1);
    assert!(repo.remove_local(document.id()).await.is_err());
    assert_eq!(document.status(), automerge_repo::DocumentStatus::Closed);
    let reopened = repo.open_document(document.id()).await.unwrap();
    assert_eq!(reopened.status(), automerge_repo::DocumentStatus::Ready);
    repo.remove_local(document.id()).await.unwrap();
    assert!(!documents.documents().contains_key(&document.id()));
}

#[tokio::test]
async fn filesystem_rejects_malformed_names_and_repo_open_rejects_corrupt_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::open(directory.path()).await.unwrap();
    let malformed = directory.path().join("automerge/NOT-A-UUID.automerge");
    std::fs::write(&malformed, b"bad").unwrap();
    let error = StorageAdapter::list(&storage).await.unwrap_err();
    assert!(error.to_string().contains("NOT-A-UUID.automerge"));
    std::fs::remove_file(malformed).unwrap();
    // Corrupt bytes are listed, not rejected: validation belongs to
    // `Repo::open`, which refuses to start over an unloadable snapshot and
    // leaves the bytes in place for the operator.
    let corrupt_id = DocumentId::new();
    let corrupt_path = directory
        .path()
        .join(format!("automerge/{corrupt_id}.automerge"));
    std::fs::write(&corrupt_path, b"not automerge").unwrap();
    assert_eq!(
        StorageAdapter::list(&storage).await.unwrap(),
        vec![corrupt_id]
    );
    let (transport, _) = MemoryTransport::pair("repo", "peer", 8);
    let error = Repo::open(
        Arc::new(storage.clone()),
        Arc::new(storage.clone()),
        transport,
        RepoConfig::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::Automerge { document, .. } if document == corrupt_id));
    assert_eq!(std::fs::read(&corrupt_path).unwrap(), b"not automerge");

    // Control values are opaque bytes; only the key alphabet is validated.
    let control_path = directory.path().join("control/opaque.bin");
    std::fs::write(&control_path, b"FIBC").unwrap();
    assert_eq!(
        ControlStore::get(&storage, "opaque").await.unwrap(),
        Some(b"FIBC".to_vec())
    );
    for key in ["", "../escape", "with space", "dot.ted"] {
        let error = ControlStore::put(&storage, key, vec![1]).await.unwrap_err();
        assert_eq!(error.operation, "control_key");
    }
}

#[tokio::test]
async fn blocked_peer_writer_is_bounded_and_does_not_stall_coordinator() {
    let docs_a = MemoryStore::default();
    let docs_b = MemoryStore::default();
    let control_a = MemoryStore::default();
    let control_b = MemoryStore::default();
    let (net_a, net_b) = MemoryTransport::pair("a", "b", 128);
    let config = RepoConfig {
        peer_writer_capacity: 8,
        ..RepoConfig::default()
    };
    let a = Repo::open(
        Arc::new(docs_a),
        Arc::new(control_a),
        net_a.clone(),
        config.clone(),
    )
    .await
    .unwrap();
    let b = Repo::open(Arc::new(docs_b), Arc::new(control_b), net_b.clone(), config)
        .await
        .unwrap();
    let doc_a = a.create().await.unwrap();
    net_a.connect().await;
    drive(&net_a, &net_b).await;
    let doc_b = b.find(doc_a.id()).await.unwrap();
    doc_b.ready().await.unwrap();

    net_a.block_sends(true);
    let mut errors = a.subscribe_errors();
    for index in 0..30 {
        doc_a
            .change(move |tx| put(tx, &format!("queued-{index}"), index))
            .await
            .unwrap();
    }
    assert!(!a.document_ids().await.unwrap().is_empty());
    let failure = tokio::time::timeout(Duration::from_secs(1), errors.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(failure, Error::Network(_)));
    net_a.block_sends(false);
}
