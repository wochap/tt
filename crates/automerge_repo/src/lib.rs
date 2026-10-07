#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod document;
pub mod error;
pub mod ids;
mod lifecycle;
pub mod network;
pub mod policy;
pub mod protocol;
pub mod repo;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod storage;
pub mod sync;
#[doc(hidden)]
pub mod testing;
pub mod transport;

pub use document::{ChangeOrigin, ChangeResult, DocHandle, DocumentEvent, DocumentStatus};
pub use error::{Error, Failure, FailurePhase, Result};
pub use ids::{DocumentId, InvalidDocumentId, PeerId};
pub use policy::{AccessPolicy, AllowAll, FnPolicy};
pub use repo::{Repo, RepoConfig};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStorage;
pub use storage::FilesystemStorage;
pub use sync::{PeerSyncProgress, PeerSyncState, RelationshipSyncState};
