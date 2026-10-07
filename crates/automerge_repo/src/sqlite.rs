//! SQLite implementation of both persistence ports in one file.
//!
//! Schema: `documents(key TEXT PRIMARY KEY, bytes BLOB)` holds complete
//! Automerge snapshots keyed by hyphenated document UUID; `control(key TEXT
//! PRIMARY KEY, bytes BLOB)` holds control values. Each `store`/`put` is one
//! `INSERT OR REPLACE` statement in its own transaction, so a key is always
//! either its previous or its new value. The database runs in WAL mode with
//! `synchronous=FULL`, making every committed replace durable.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    DocumentId,
    error::StorageError,
    storage::{ControlStore, StorageAdapter},
};

#[derive(Clone)]
pub struct SqliteStorage {
    connection: Arc<Mutex<Connection>>,
    path: Arc<PathBuf>,
    documents_closed: Arc<AtomicBool>,
    control_closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for SqliteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStorage")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl SqliteStorage {
    /// Opens (creating if needed) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let error = |message: String| StorageError::new("open", None, message).with_path(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| error(e.to_string()))?;
        }
        let connection = Connection::open(&path).map_err(|e| error(e.to_string()))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=FULL;
                 PRAGMA busy_timeout=5000;
                 CREATE TABLE IF NOT EXISTS documents (key TEXT PRIMARY KEY, bytes BLOB NOT NULL);
                 CREATE TABLE IF NOT EXISTS control (key TEXT PRIMARY KEY, bytes BLOB NOT NULL);",
            )
            .map_err(|e| error(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            path: Arc::new(path),
            documents_closed: Arc::new(AtomicBool::new(false)),
            control_closed: Arc::new(AtomicBool::new(false)),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: &'static str,
        document: Option<DocumentId>,
        job: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T, StorageError> {
        let closed = if operation.starts_with("control_") {
            &self.control_closed
        } else {
            &self.documents_closed
        };
        if closed.load(Ordering::Acquire) {
            return Err(StorageError::new(operation, document, "adapter is closed"));
        }
        let connection = self.connection.clone();
        let path = self.path.as_ref().clone();
        tokio::task::spawn_blocking(move || {
            let connection = connection.lock().unwrap_or_else(|e| e.into_inner());
            job(&connection)
        })
        .await
        .map_err(|error| StorageError::new(operation, document, error.to_string()))?
        .map_err(|error| StorageError::new(operation, document, error.to_string()).with_path(path))
    }
}

#[async_trait]
impl StorageAdapter for SqliteStorage {
    async fn list(&self) -> Result<Vec<DocumentId>, StorageError> {
        let keys = self
            .run("list", None, |connection| {
                let mut statement = connection.prepare("SELECT key FROM documents ORDER BY key")?;
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await?;
        let mut ids = keys
            .into_iter()
            .map(|key| {
                key.parse::<DocumentId>().map_err(|_| {
                    StorageError::new("list", None, format!("malformed document key {key:?}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        ids.sort();
        Ok(ids)
    }

    async fn load(&self, id: DocumentId) -> Result<Option<Vec<u8>>, StorageError> {
        self.run("load", Some(id), move |connection| {
            connection
                .query_row(
                    "SELECT bytes FROM documents WHERE key = ?1",
                    params![id.to_string()],
                    |row| row.get(0),
                )
                .optional()
        })
        .await
    }

    async fn store(&self, id: DocumentId, snapshot: Vec<u8>) -> Result<(), StorageError> {
        self.run("store", Some(id), move |connection| {
            connection
                .execute(
                    "INSERT OR REPLACE INTO documents (key, bytes) VALUES (?1, ?2)",
                    params![id.to_string(), snapshot],
                )
                .map(|_| ())
        })
        .await
    }

    async fn remove(&self, id: DocumentId) -> Result<(), StorageError> {
        self.run("remove", Some(id), move |connection| {
            connection
                .execute(
                    "DELETE FROM documents WHERE key = ?1",
                    params![id.to_string()],
                )
                .map(|_| ())
        })
        .await
    }

    async fn flush(&self) -> Result<(), StorageError> {
        // Every statement commits with synchronous=FULL; nothing is buffered.
        self.run("document_flush", None, |_| Ok(())).await
    }

    async fn close(&self) -> Result<(), StorageError> {
        self.documents_closed.store(true, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl ControlStore for SqliteStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let key = key.to_owned();
        self.run("control_load", None, move |connection| {
            connection
                .query_row(
                    "SELECT bytes FROM control WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()
        })
        .await
    }

    async fn put(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let key = key.to_owned();
        self.run("control_store", None, move |connection| {
            connection
                .execute(
                    "INSERT OR REPLACE INTO control (key, bytes) VALUES (?1, ?2)",
                    params![key, value],
                )
                .map(|_| ())
        })
        .await
    }

    async fn flush(&self) -> Result<(), StorageError> {
        self.run("control_flush", None, |_| Ok(())).await
    }

    async fn close(&self) -> Result<(), StorageError> {
        self.control_closed.store(true, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_replace_remove_and_control() {
        let directory = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::open(directory.path().join("tt.db")).unwrap();
        let id = DocumentId::new();
        assert_eq!(StorageAdapter::load(&storage, id).await.unwrap(), None);
        storage.store(id, vec![1]).await.unwrap();
        storage.store(id, vec![2, 3]).await.unwrap();
        assert_eq!(storage.load(id).await.unwrap(), Some(vec![2, 3]));
        assert_eq!(storage.list().await.unwrap(), vec![id]);
        storage.put("storage-id", b"abc".to_vec()).await.unwrap();
        assert_eq!(
            storage.get("storage-id").await.unwrap(),
            Some(b"abc".to_vec())
        );
        storage.remove(id).await.unwrap();
        assert!(storage.list().await.unwrap().is_empty());
        StorageAdapter::close(&storage).await.unwrap();
        assert!(storage.load(id).await.is_err());
        assert!(storage.get("storage-id").await.is_ok());
    }
}
