//! Per-user document visibility, derived from the registry and the users'
//! index documents.
//!
//! A user may sync its own index document and every document that index
//! lists. When several indexes list one document, the user whose account
//! was created first (ties: lower id) owns it and the conflict is logged
//! once. The registry document is never owned by anyone. [`AccessIndex`]
//! keeps the derived map in memory so the repository's [`AccessPolicy`]
//! (called on its coordinator, must not block) answers without I/O; the
//! server updates it when the registry or an index changes.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use automerge_repo::{AccessPolicy, DocumentId, PeerId};
use tracing::warn;

/// What a user may do with a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The user owns the document.
    Granted,
    /// Another user owns it, or it is the registry.
    Foreign,
    /// No index lists it: a document the server has not accepted.
    Unlisted,
}

/// The user id inside a repository peer id. Sessions use the identity
/// `<user id>.<nonce>`, and `WsJsServer` scopes it as `<identity>/<senderId>`.
#[must_use]
pub fn user_of(peer: &PeerId) -> Option<&str> {
    let (identity, _) = peer.as_str().split_once('/')?;
    let (user, _) = identity.split_once('.')?;
    Some(user)
}

/// An account as the access index needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub created: i64,
    pub index: DocumentId,
}

#[derive(Default)]
struct Inner {
    registry: Option<DocumentId>,
    members: HashMap<String, Member>,
    listings: HashMap<String, HashSet<DocumentId>>,
    owners: HashMap<DocumentId, String>,
    /// Conflicts already logged, as (document, losing user).
    logged: HashSet<(DocumentId, String)>,
}

impl Inner {
    fn recompute(&mut self) {
        let mut members: Vec<&Member> = self.members.values().collect();
        members.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        let mut owners: HashMap<DocumentId, String> = HashMap::new();
        // Index documents belong to their account first.
        for member in &members {
            owners
                .entry(member.index)
                .or_insert_with(|| member.id.clone());
        }
        let mut conflicts = Vec::new();
        for member in &members {
            let Some(listed) = self.listings.get(&member.id) else {
                continue;
            };
            let mut listed: Vec<&DocumentId> = listed.iter().collect();
            listed.sort();
            for doc in listed {
                match owners.get(doc) {
                    None => {
                        owners.insert(*doc, member.id.clone());
                    }
                    Some(owner) if owner != &member.id => {
                        conflicts.push((*doc, owner.clone(), member.id.clone()));
                    }
                    Some(_) => {}
                }
            }
        }
        if let Some(registry) = self.registry {
            owners.remove(&registry);
        }
        for (doc, owner, loser) in conflicts {
            if self.logged.insert((doc, loser.clone())) {
                warn!(
                    document = %doc.to_bs58check(),
                    %owner,
                    user = %loser,
                    "two indexes list one document; the earlier account keeps it"
                );
            }
        }
        self.owners = owners;
    }
}

#[derive(Default)]
pub struct AccessIndex {
    inner: RwLock<Inner>,
}

impl AccessIndex {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The registry document, which nobody may sync.
    pub fn set_registry(&self, registry: Option<DocumentId>) {
        let mut inner = self.inner.write().unwrap();
        inner.registry = registry;
        inner.recompute();
    }

    /// Replaces the set of non-deleted accounts. Listings of accounts that
    /// are gone are dropped; returns the members without a listing yet.
    pub fn set_members(&self, members: Vec<Member>) -> Vec<Member> {
        let mut inner = self.inner.write().unwrap();
        inner.members = members.into_iter().map(|m| (m.id.clone(), m)).collect();
        let Inner {
            members, listings, ..
        } = &mut *inner;
        listings.retain(|user, _| members.contains_key(user));
        let missing = members
            .values()
            .filter(|member| !listings.contains_key(&member.id))
            .cloned()
            .collect();
        inner.recompute();
        missing
    }

    /// Records the documents `user`'s index currently lists. Ignored for
    /// users that are not members.
    pub fn set_listing(&self, user: &str, listed: impl IntoIterator<Item = DocumentId>) {
        let mut inner = self.inner.write().unwrap();
        if !inner.members.contains_key(user) {
            return;
        }
        let listed: HashSet<DocumentId> = listed.into_iter().collect();
        if inner.listings.get(user) == Some(&listed) {
            return;
        }
        inner.listings.insert(user.to_owned(), listed);
        inner.recompute();
    }

    #[must_use]
    pub fn member(&self, user: &str) -> Option<Member> {
        self.inner.read().unwrap().members.get(user).cloned()
    }

    #[must_use]
    pub fn owner(&self, doc: DocumentId) -> Option<String> {
        self.inner.read().unwrap().owners.get(&doc).cloned()
    }

    #[must_use]
    pub fn access(&self, doc: DocumentId, user: &str) -> Access {
        let inner = self.inner.read().unwrap();
        if inner.registry == Some(doc) {
            return Access::Foreign;
        }
        match inner.owners.get(&doc) {
            Some(owner) if owner == user => Access::Granted,
            Some(_) => Access::Foreign,
            None => Access::Unlisted,
        }
    }

    /// Documents `user` owns, sorted.
    #[must_use]
    pub fn owned(&self, user: &str) -> Vec<DocumentId> {
        let inner = self.inner.read().unwrap();
        let mut owned: Vec<DocumentId> = inner
            .owners
            .iter()
            .filter(|(_, owner)| *owner == user)
            .map(|(doc, _)| *doc)
            .collect();
        owned.sort();
        owned
    }
}

/// [`AccessPolicy`] over the index: a peer sees exactly the documents its
/// user owns. Peers without a user identity see nothing.
pub struct DerivedPolicy(pub Arc<AccessIndex>);

impl AccessPolicy for DerivedPolicy {
    fn may_sync(&self, peer: &PeerId, document: DocumentId) -> bool {
        user_of(peer).is_some_and(|user| self.0.access(document, user) == Access::Granted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, created: i64) -> Member {
        Member {
            id: id.into(),
            created,
            index: DocumentId::new(),
        }
    }

    #[test]
    fn peer_ids_carry_the_user() {
        let peer = PeerId::from("0b5c-uuid.1f2e3d/tt-device");
        assert_eq!(user_of(&peer), Some("0b5c-uuid"));
        assert_eq!(user_of(&PeerId::from("tt-device")), None);
        assert_eq!(user_of(&PeerId::from("no-nonce/tt-device")), None);
    }

    #[test]
    fn users_own_their_index_and_what_it_lists() {
        let index = AccessIndex::new();
        let (alice, bob) = (member("alice", 1), member("bob", 2));
        let missing = index.set_members(vec![alice.clone(), bob.clone()]);
        assert_eq!(missing.len(), 2);
        let workspace = DocumentId::new();
        index.set_listing("alice", [workspace]);
        assert_eq!(index.access(alice.index, "alice"), Access::Granted);
        assert_eq!(index.access(workspace, "alice"), Access::Granted);
        assert_eq!(index.owned("alice").len(), 2);
        // Cross-user request.
        assert_eq!(index.access(workspace, "bob"), Access::Foreign);
        assert_eq!(index.access(alice.index, "bob"), Access::Foreign);
        assert_eq!(index.access(DocumentId::new(), "alice"), Access::Unlisted);

        let policy = DerivedPolicy(index.clone());
        assert!(policy.may_sync(&PeerId::from("alice.1/dev"), workspace));
        assert!(!policy.may_sync(&PeerId::from("bob.1/dev"), workspace));
        assert!(!policy.may_sync(&PeerId::from("dev"), workspace));

        // A deleted account (dropped from the members) has access to nothing.
        index.set_members(vec![bob]);
        assert_eq!(index.access(workspace, "alice"), Access::Unlisted);
        assert_eq!(index.access(alice.index, "alice"), Access::Unlisted);
    }

    #[test]
    fn two_indexes_listing_one_document_keep_the_earlier_account() {
        let index = AccessIndex::new();
        let (alice, bob) = (member("alice", 1), member("bob", 2));
        index.set_members(vec![alice.clone(), bob.clone()]);
        let shared = DocumentId::new();
        // Bob lists it first, then Alice: Alice was created first and wins.
        index.set_listing("bob", [shared, alice.index]);
        assert_eq!(index.access(shared, "bob"), Access::Granted);
        index.set_listing("alice", [shared]);
        assert_eq!(index.access(shared, "alice"), Access::Granted);
        assert_eq!(index.access(shared, "bob"), Access::Foreign);
        assert_eq!(
            index.access(alice.index, "bob"),
            Access::Foreign,
            "an index belongs to its account"
        );
    }

    #[test]
    fn the_registry_belongs_to_nobody() {
        let index = AccessIndex::new();
        let alice = member("alice", 1);
        index.set_members(vec![alice]);
        let registry = DocumentId::new();
        index.set_registry(Some(registry));
        index.set_listing("alice", [registry]);
        assert_eq!(index.access(registry, "alice"), Access::Foreign);
        assert_eq!(index.owner(registry), None);
    }
}
