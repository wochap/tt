//! Per-user document visibility.
//!
//! The `acl` table is authoritative. [`AclCache`] mirrors the rows of every
//! document a connection has touched so the repository's [`AccessPolicy`]
//! (called on its coordinator, must not block) answers from memory. The sync
//! gate in front of each websocket refreshes the cache from the database
//! before a frame reaches the repository, so rows written by another process
//! (`tt-server user add`) are seen.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use automerge_repo::{AccessPolicy, DocumentId, PeerId};

use crate::db::Db;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Writer,
    Reader,
}

impl Role {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "owner" => Self::Owner,
            "writer" => Self::Writer,
            _ => Self::Reader,
        }
    }
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Writer => "writer",
            Self::Reader => "reader",
        }
    }
    #[must_use]
    pub const fn may_write(self) -> bool {
        matches!(self, Self::Owner | Self::Writer)
    }
}

/// What a user may do with a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Granted(Role),
    /// Other users hold rows for it; this user does not.
    Foreign,
    /// Nobody holds a row: a document the server has never accepted.
    Unknown,
}

/// The user id inside a repository peer id. Sessions use the identity
/// `<user id>.<nonce>`, and `WsJsServer` scopes it as `<identity>/<senderId>`.
#[must_use]
pub fn user_of(peer: &PeerId) -> Option<&str> {
    let (identity, _) = peer.as_str().split_once('/')?;
    let (user, _) = identity.split_once('.')?;
    Some(user)
}

type Rows = Vec<(String, Role)>;

pub struct AclCache {
    db: Db,
    rows: RwLock<HashMap<DocumentId, Rows>>,
}

impl AclCache {
    #[must_use]
    pub fn new(db: Db) -> Arc<Self> {
        Arc::new(Self {
            db,
            rows: RwLock::new(HashMap::new()),
        })
    }

    /// Memory-only lookup.
    #[must_use]
    pub fn cached(&self, doc: DocumentId, user: &str) -> Option<Role> {
        self.rows.read().unwrap().get(&doc).and_then(|rows| {
            rows.iter()
                .find(|(id, _)| id == user)
                .map(|(_, role)| *role)
        })
    }

    fn evaluate(rows: &Rows, user: &str) -> Access {
        match rows.iter().find(|(id, _)| id == user) {
            Some((_, role)) => Access::Granted(*role),
            None if rows.is_empty() => Access::Unknown,
            None => Access::Foreign,
        }
    }

    fn store(&self, doc: DocumentId, rows: Rows) {
        let mut cache = self.rows.write().unwrap();
        if rows.is_empty() {
            cache.remove(&doc);
        } else {
            cache.insert(doc, rows);
        }
    }

    /// Cache first; on a miss the rows are reloaded from the database.
    pub async fn access(&self, doc: DocumentId, user: &str) -> Result<Access> {
        if let Some(role) = self.cached(doc, user) {
            return Ok(Access::Granted(role));
        }
        let rows = self.db.run(move |db| db.acl_rows(doc)).await?;
        let access = Self::evaluate(&rows, user);
        self.store(doc, rows);
        Ok(access)
    }

    /// Makes `user` the owner of an unowned document; returns the resulting
    /// access (still `Foreign` if someone else won a race).
    pub async fn claim(&self, doc: DocumentId, user: &str) -> Result<Access> {
        let owner = user.to_owned();
        let rows = self.db.run(move |db| db.claim(doc, &owner)).await?;
        let access = Self::evaluate(&rows, user);
        self.store(doc, rows);
        Ok(access)
    }
}

/// [`AccessPolicy`] over the cache: a peer sees exactly the documents its
/// user holds a role on. Peers without a user identity see nothing.
pub struct AclPolicy(pub Arc<AclCache>);

impl AccessPolicy for AclPolicy {
    fn may_sync(&self, peer: &PeerId, document: DocumentId) -> bool {
        user_of(peer).is_some_and(|user| self.0.cached(document, user).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_ids_carry_the_user() {
        let peer = PeerId::from("0b5c-uuid.1f2e3d/tt-device");
        assert_eq!(user_of(&peer), Some("0b5c-uuid"));
        assert_eq!(user_of(&PeerId::from("tt-device")), None);
        assert_eq!(user_of(&PeerId::from("no-nonce/tt-device")), None);
    }
}
