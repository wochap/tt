//! `SqliteStorage` durability: a writer SIGKILLed mid-write never leaves a
//! half-written snapshot, and a `Repo` over SQLite survives restart.
#![cfg(feature = "sqlite")]

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use automerge::{Automerge, ROOT, ReadDoc, ScalarValue, transaction::Transactable};
use automerge_repo::{
    DocumentId, DocumentStatus, Error, Repo, RepoConfig, SqliteStorage,
    storage::{ControlStore, StorageAdapter},
    testing::MemoryTransport,
};

const CHILD_ENV: &str = "TT_SQLITE_CRASH_CHILD_DB";
const CHILD_TEST: &str = "crash_writer_child";
const KEY_A: DocumentId = DocumentId::from_bytes([0xa; 16]);
const KEY_B: DocumentId = DocumentId::from_bytes([0xb; 16]);
const BLOB_LEN: usize = 512 * 1024;
/// Acknowledged B writes to wait for before killing the writer.
const WRITES_BEFORE_KILL: u64 = 8;

/// Deterministic, incompressible bytes for iteration `round`.
fn blob(round: u64) -> Vec<u8> {
    let mut state = round.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    (0..BLOB_LEN)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn snapshot(round: u64) -> Vec<u8> {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        tx.put(ROOT, "round", round)?;
        tx.put(ROOT, "blob", ScalarValue::Bytes(blob(round)))?;
        Ok(())
    })
    .unwrap();
    doc.save()
}

fn read_round(doc: &Automerge) -> (u64, Vec<u8>) {
    let round = match doc.get(ROOT, "round").unwrap().unwrap().0 {
        automerge::Value::Scalar(value) => match value.as_ref() {
            ScalarValue::Uint(round) => *round,
            other => panic!("unexpected round {other:?}"),
        },
        other => panic!("unexpected round {other:?}"),
    };
    let bytes = match doc.get(ROOT, "blob").unwrap().unwrap().0 {
        automerge::Value::Scalar(value) => match value.as_ref() {
            ScalarValue::Bytes(bytes) => bytes.clone(),
            other => panic!("unexpected blob {other:?}"),
        },
        other => panic!("unexpected blob {other:?}"),
    };
    (round, bytes)
}

/// Child mode: only does work when re-executed by the parent test with
/// `TT_SQLITE_CRASH_CHILD_DB` set. Writes A once, then rewrites B forever,
/// printing one acknowledgement line after each committed write.
#[tokio::test]
async fn crash_writer_child() {
    let Ok(path) = std::env::var(CHILD_ENV) else {
        return;
    };
    let storage = SqliteStorage::open(&path).unwrap();
    storage.store(KEY_A, snapshot(u64::MAX)).await.unwrap();
    StorageAdapter::flush(&storage).await.unwrap();
    println!("ACK A");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut round = 0_u64;
    while Instant::now() < deadline {
        storage.store(KEY_B, snapshot(round)).await.unwrap();
        println!("ACK B {round}");
        round += 1;
    }
    panic!("crash writer was never killed");
}

fn spawn_writer(path: &Path) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[tokio::test]
async fn sigkilled_writer_leaves_only_complete_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let path: PathBuf = directory.path().join("crash.db");
    let mut child = spawn_writer(&path);
    let lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut saw_a = false;
    let mut last_b = None::<u64>;
    for line in lines {
        let line = line.unwrap();
        if line.contains("ACK A") {
            saw_a = true;
        } else if let Some(round) = line.split("ACK B ").nth(1) {
            let round: u64 = round.trim().parse().unwrap();
            last_b = Some(round);
            if round + 1 >= WRITES_BEFORE_KILL {
                break;
            }
        }
    }
    assert!(saw_a, "child must acknowledge A before any B");
    // `Child::kill` is SIGKILL on Unix: no destructors, no SQLite cleanup,
    // most likely in the middle of the next B write.
    child.kill().unwrap();
    child.wait().unwrap();
    let acknowledged = last_b.expect("child acknowledged B writes");

    let storage = SqliteStorage::open(&path).unwrap();
    assert_eq!(storage.list().await.unwrap(), vec![KEY_A, KEY_B]);
    let a = Automerge::load(&storage.load(KEY_A).await.unwrap().unwrap()).unwrap();
    assert_eq!(read_round(&a), (u64::MAX, blob(u64::MAX)));
    let b = Automerge::load(&storage.load(KEY_B).await.unwrap().unwrap()).unwrap();
    let (round, bytes) = read_round(&b);
    // The kill may land after a commit but before its acknowledgement line
    // was read, so B is the last acknowledged round or the one after it.
    assert!(
        round == acknowledged || round == acknowledged + 1,
        "B is round {round}, last acknowledged {acknowledged}"
    );
    assert_eq!(bytes, blob(round), "B is exactly one complete snapshot");

    // The repository's strict open accepts the recovered database.
    let (transport, _) = MemoryTransport::pair("recovered", "peer", 8);
    let repo = Repo::open(
        Arc::new(storage.clone()),
        Arc::new(storage),
        transport,
        RepoConfig::default(),
    )
    .await
    .unwrap();
    assert_eq!(repo.document_ids().await.unwrap(), vec![KEY_A, KEY_B]);
    repo.shutdown().await.unwrap();
}

fn put(
    tx: &mut automerge::transaction::Transaction<'_>,
    key: &str,
    value: i64,
) -> automerge_repo::Result<()> {
    tx.put(ROOT, key, value)
        .map_err(|error| Error::Change(error.to_string()))
}

async fn open_repo(path: &Path, name: &str) -> (Repo, SqliteStorage) {
    let storage = SqliteStorage::open(path).unwrap();
    let (transport, _) = MemoryTransport::pair(name, "unused", 8);
    let repo = Repo::open(
        Arc::new(storage.clone()),
        Arc::new(storage.clone()),
        transport,
        RepoConfig::default(),
    )
    .await
    .unwrap();
    (repo, storage)
}

#[tokio::test]
async fn repository_over_sqlite_survives_flush_shutdown_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/repo.db");
    let (first, storage) = open_repo(&path, "first").await;
    storage
        .put("storage-id", b"first-storage".to_vec())
        .await
        .unwrap();
    let document = first.create_with(|tx| put(tx, "created", 1)).await.unwrap();
    document.change(|tx| put(tx, "mutated", 2)).await.unwrap();
    let removed = first.create().await.unwrap();
    first.remove_local(removed.id()).await.unwrap();
    first.flush().await.unwrap();
    let heads_at_flush = document.read(|doc| doc.get_heads()).await.unwrap();
    // A change after flush is still persisted by shutdown's final flush.
    document
        .change(|tx| put(tx, "at-shutdown", 3))
        .await
        .unwrap();
    let heads_at_shutdown = document.read(|doc| doc.get_heads()).await.unwrap();
    first.shutdown().await.unwrap();
    assert!(
        storage.load(document.id()).await.is_err(),
        "closed on shutdown"
    );
    drop(storage);

    let (second, storage) = open_repo(&path, "second").await;
    assert_eq!(second.document_ids().await.unwrap(), vec![document.id()]);
    let reopened = second.open_document(document.id()).await.unwrap();
    assert_eq!(reopened.status(), DocumentStatus::Ready);
    let (heads, values) = reopened
        .read(|doc| {
            let value = |key: &str| {
                doc.get(ROOT, key)
                    .unwrap()
                    .and_then(|(value, _)| value.to_i64())
            };
            (
                doc.get_heads(),
                [value("created"), value("mutated"), value("at-shutdown")],
            )
        })
        .await
        .unwrap();
    assert_eq!(heads, heads_at_shutdown);
    assert_ne!(heads, heads_at_flush);
    assert_eq!(values, [Some(1), Some(2), Some(3)]);
    assert_eq!(
        storage.get("storage-id").await.unwrap(),
        Some(b"first-storage".to_vec())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    second.shutdown().await.unwrap();
}
