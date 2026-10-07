//! Access policy port: decides which documents a peer may see.

use crate::{DocumentId, PeerId};

/// Consulted by the repository before it announces a document to a peer,
/// services a peer's `request`, or accepts or sends `sync` data for a
/// document. A denied document is answered with `doc-unavailable` and no sync
/// data is ever sent for it.
///
/// Calls happen on the repository coordinator, so implementations must be
/// fast and must not block; keep lookups in memory.
pub trait AccessPolicy: Send + Sync + 'static {
    /// Whether `peer` may exchange sync data for `document` at all.
    fn may_sync(&self, peer: &PeerId, document: DocumentId) -> bool;

    /// Whether the repository should push `document` to `peer` unprompted
    /// (on connect or on creation). Defaults to [`Self::may_sync`]; a server
    /// that only answers explicit requests returns `false`.
    fn may_announce(&self, peer: &PeerId, document: DocumentId) -> bool {
        self.may_sync(peer, document)
    }
}

/// Default policy: every peer may see every document.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAll;

impl AccessPolicy for AllowAll {
    fn may_sync(&self, _: &PeerId, _: DocumentId) -> bool {
        true
    }
}

/// Closure-backed policy, mainly for tests and simple servers.
pub struct FnPolicy<F>(pub F);

impl<F> AccessPolicy for FnPolicy<F>
where
    F: Fn(&PeerId, DocumentId) -> bool + Send + Sync + 'static,
{
    fn may_sync(&self, peer: &PeerId, document: DocumentId) -> bool {
        (self.0)(peer, document)
    }
}
