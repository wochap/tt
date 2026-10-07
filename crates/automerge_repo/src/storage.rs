//! Complete-snapshot and control-state persistence ports and filesystem adapter.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use async_trait::async_trait;

use crate::{DocumentId, error::StorageError};

#[async_trait]
/// Atomic complete-snapshot storage. `store` installs bytes atomically; `flush`
/// is the explicit crash-durability barrier for preceding operations.
pub trait StorageAdapter: Send + Sync + 'static {
    /// Lists stored snapshot IDs without validating their contents; strict
    /// validation happens in `Repo::open`.
    async fn list(&self) -> Result<Vec<DocumentId>, StorageError>;
    async fn load(&self, id: DocumentId) -> Result<Option<Vec<u8>>, StorageError>;
    async fn store(&self, id: DocumentId, snapshot: Vec<u8>) -> Result<(), StorageError>;
    async fn remove(&self, id: DocumentId) -> Result<(), StorageError>;
    async fn flush(&self) -> Result<(), StorageError>;
    async fn close(&self) -> Result<(), StorageError>;
}

#[async_trait]
/// Small keyed control state kept apart from document bytes (for example the
/// repository storage id). Each `put` replaces its key atomically; a
/// successful put is durable only after the following `flush`.
pub trait ControlStore: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn put(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError>;
    async fn flush(&self) -> Result<(), StorageError>;
    async fn close(&self) -> Result<(), StorageError>;
}

/// Crash-safe filesystem implementation of both repository persistence ports.
///
/// Documents live in `automerge/<uuid>.automerge`; control keys live in
/// `control/<key>.bin`. Files not ending in `.automerge` are unrelated and
/// ignored, except recognizable `.tmp-<hex>` sibling files left by an
/// interrupted adapter write, which are removed during listing.
#[derive(Clone, Debug)]
pub struct FilesystemStorage {
    root: Arc<PathBuf>,
    documents_closed: Arc<AtomicBool>,
    control_closed: Arc<AtomicBool>,
    nonce: Arc<AtomicU64>,
}

impl FilesystemStorage {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(path.into()),
            documents_closed: Arc::new(AtomicBool::new(false)),
            control_closed: Arc::new(AtomicBool::new(false)),
            nonce: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn open(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let storage = Self::new(path);
        let root = storage.root.as_ref().clone();
        blocking("open", None, None, move || {
            ensure_directory(&root)?;
            ensure_directory(&root.join("automerge"))?;
            ensure_directory(&root.join("control"))
        })
        .await?;
        Ok(storage)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    fn documents_dir(&self) -> PathBuf {
        self.root.join("automerge")
    }

    fn control_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(StorageError::new(
                "control_key",
                None,
                format!("invalid control key {key:?}"),
            ));
        }
        Ok(self.root.join("control").join(format!("{key}.bin")))
    }

    fn ensure_open(
        &self,
        operation: &'static str,
        document: Option<DocumentId>,
    ) -> Result<(), StorageError> {
        let closed = if operation.starts_with("control_") {
            self.control_closed.load(Ordering::Acquire)
        } else {
            self.documents_closed.load(Ordering::Acquire)
        };
        if closed {
            Err(StorageError::new(operation, document, "adapter is closed"))
        } else {
            Ok(())
        }
    }

    fn document_path(&self, id: DocumentId) -> PathBuf {
        self.documents_dir().join(format!("{id}.automerge"))
    }

    async fn replace(
        &self,
        operation: &'static str,
        document: Option<DocumentId>,
        path: PathBuf,
        bytes: Vec<u8>,
    ) -> Result<(), StorageError> {
        self.ensure_open(operation, document)?;
        let nonce = self.nonce.fetch_add(1, Ordering::Relaxed);
        blocking(operation, document, Some(path.clone()), move || {
            atomic_replace(&path, &bytes, nonce)
        })
        .await
    }
}

async fn blocking<T: Send + 'static>(
    operation: &'static str,
    document: Option<DocumentId>,
    path: Option<PathBuf>,
    job: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Result<T, StorageError> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|error| StorageError::new(operation, document, error.to_string()))?
        .map_err(|error| {
            let storage = StorageError::new(operation, document, error.to_string());
            path.map_or(storage.clone(), |path| storage.with_path(path))
        })
}

fn ensure_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn atomic_replace(path: &Path, bytes: &[u8], nonce: u64) -> std::io::Result<()> {
    atomic_replace_with(path, bytes, nonce, |_| Ok(()))
}

fn atomic_replace_with(
    path: &Path,
    bytes: &[u8],
    nonce: u64,
    before_replace: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path.parent().expect("repository files have parents");
    ensure_directory(parent)?;
    let name = path
        .file_name()
        .expect("repository files have names")
        .to_string_lossy();
    let mut attempt = nonce;
    let (temporary, mut file) = loop {
        let candidate = parent.join(format!(".{name}.tmp-{attempt:016x}"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt = attempt.wrapping_add(1);
            }
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        before_replace(&temporary)?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn is_stale_temp(name: &str) -> bool {
    let Some((prefix, suffix)) = name.rsplit_once(".tmp-") else {
        return false;
    };
    prefix.starts_with('.')
        && (prefix.ends_with(".automerge") || prefix.ends_with(".bin"))
        && suffix.len() == 16
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[async_trait]
impl StorageAdapter for FilesystemStorage {
    async fn list(&self) -> Result<Vec<DocumentId>, StorageError> {
        self.ensure_open("list", None)?;
        let directory = self.documents_dir();
        blocking("list", None, Some(directory.clone()), move || {
            ensure_directory(&directory)?;
            let mut ids = Vec::new();
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if is_stale_temp(&name) {
                    if entry.file_type()?.is_file() {
                        fs::remove_file(entry.path())?;
                    }
                    continue;
                }
                if !name.ends_with(".automerge") {
                    continue;
                }
                let stem = name.strip_suffix(".automerge").unwrap();
                let id: DocumentId = stem.parse().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("malformed snapshot filename: {}", entry.path().display()),
                    )
                })?;
                if id.to_string() != stem {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("noncanonical snapshot filename: {}", entry.path().display()),
                    ));
                }
                ids.push(id);
            }
            ids.sort();
            Ok(ids)
        })
        .await
    }

    async fn load(&self, id: DocumentId) -> Result<Option<Vec<u8>>, StorageError> {
        self.ensure_open("load", Some(id))?;
        let path = self.document_path(id);
        blocking(
            "load",
            Some(id),
            Some(path.clone()),
            move || match fs::read(&path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            },
        )
        .await
    }

    async fn store(&self, id: DocumentId, snapshot: Vec<u8>) -> Result<(), StorageError> {
        self.replace("store", Some(id), self.document_path(id), snapshot)
            .await
    }

    async fn remove(&self, id: DocumentId) -> Result<(), StorageError> {
        self.ensure_open("remove", Some(id))?;
        let path = self.document_path(id);
        blocking(
            "remove",
            Some(id),
            Some(path.clone()),
            move || match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        )
        .await
    }

    async fn flush(&self) -> Result<(), StorageError> {
        self.ensure_open("document_flush", None)?;
        let path = self.documents_dir();
        blocking("document_flush", None, Some(path.clone()), move || {
            ensure_directory(&path)?;
            File::open(path)?.sync_all()
        })
        .await
    }

    async fn close(&self) -> Result<(), StorageError> {
        // The same cloneable concrete value may back both repository ports;
        // closing either view is therefore deliberately idempotent and does
        // not invalidate the other view while shutdown is still progressing.
        self.documents_closed.store(true, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl ControlStore for FilesystemStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.ensure_open("control_load", None)?;
        let path = self.control_path(key)?;
        blocking(
            "control_load",
            None,
            Some(path.clone()),
            move || match fs::read(&path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            },
        )
        .await
    }

    async fn put(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let path = self.control_path(key)?;
        self.replace("control_store", None, path, value).await
    }

    async fn flush(&self) -> Result<(), StorageError> {
        self.ensure_open("control_flush", None)?;
        let path = self.root.join("control");
        blocking("control_flush", None, Some(path.clone()), move || {
            ensure_directory(&path)?;
            File::open(path)?.sync_all()
        })
        .await
    }

    async fn close(&self) -> Result<(), StorageError> {
        self.control_closed.store(true, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_before_rename_preserves_destination_and_cleans_temp() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("value.automerge");
        fs::write(&path, b"old").unwrap();
        let result = atomic_replace_with(&path, b"new", 0, |temporary| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(fs::metadata(temporary)?.permissions().mode() & 0o777, 0o600);
            }
            Err(std::io::Error::other("injected before rename"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
