//! Per-user document visibility, derived from the registry and the users'
//! index documents, and the trust and reach of member servers.
//!
//! Server peers are keyed `srv:<server_id>`, set by the peer transport from
//! the key proven in the TLS handshake. Client peer ids always carry a `/`
//! (`<user>.<nonce>/<senderId>`), so no client can produce a server peer id,
//! whatever `senderId` it sends. A trusted server peer may sync the registry
//! closure: the registry, every account's index, and everything an index
//! lists.
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
use tokio::sync::watch;
use tracing::warn;

use crate::identity::server_id;

/// Prefix of server peer ids.
pub const SERVER_PEER_PREFIX: &str = "srv:";

/// The repository peer id of a member server.
#[must_use]
pub fn server_peer(server_id: &str) -> PeerId {
    PeerId::from(format!("{SERVER_PEER_PREFIX}{server_id}"))
}

/// The server id of a server peer id; `None` for client peers.
#[must_use]
pub fn server_of(peer: &PeerId) -> Option<&str> {
    peer.as_str()
        .strip_prefix(SERVER_PEER_PREFIX)
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

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
    if server_of(peer).is_some() {
        return None;
    }
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
    /// Deleted accounts own nothing, but their documents still replicate.
    pub active: bool,
}

#[derive(Default)]
struct Inner {
    registry: Option<DocumentId>,
    members: HashMap<String, Member>,
    listings: HashMap<String, HashSet<DocumentId>>,
    owners: HashMap<DocumentId, String>,
    /// Conflicts already logged, as (document, losing user).
    logged: HashSet<(DocumentId, String)>,
    /// The registry closure.
    closure: HashSet<DocumentId>,
    /// Non-revoked member servers other than this one: id to public key.
    servers: HashMap<String, Vec<u8>>,
    /// The server a join is fetching the root from, trusted by id until the
    /// join completes.
    joining: Option<String>,
}

impl Inner {
    fn recompute(&mut self) -> bool {
        let mut closure: HashSet<DocumentId> = self.registry.into_iter().collect();
        for member in self.members.values() {
            closure.insert(member.index);
        }
        for listed in self.listings.values() {
            closure.extend(listed.iter().copied());
        }
        let changed = closure != self.closure;
        self.closure = closure;
        let mut members: Vec<&Member> = self.members.values().filter(|m| m.active).collect();
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
        changed
    }
}

pub struct AccessIndex {
    inner: RwLock<Inner>,
    /// Bumped whenever the closure changes.
    closure_version: watch::Sender<u64>,
}

impl Default for AccessIndex {
    fn default() -> Self {
        Self {
            inner: RwLock::default(),
            closure_version: watch::Sender::new(0),
        }
    }
}

impl AccessIndex {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn update(&self, change: impl FnOnce(&mut Inner)) {
        let mut inner = self.inner.write().unwrap();
        change(&mut inner);
        if inner.recompute() {
            self.closure_version.send_modify(|version| *version += 1);
        }
    }

    /// The registry document, which no user may sync.
    pub fn set_registry(&self, registry: Option<DocumentId>) {
        self.update(|inner| inner.registry = registry);
    }

    /// Replaces the set of non-deleted accounts. Listings of accounts that
    /// are gone are dropped; returns the members without a listing yet.
    pub fn set_members(&self, members: Vec<Member>) -> Vec<Member> {
        let mut missing = Vec::new();
        self.update(|inner| {
            inner.members = members.into_iter().map(|m| (m.id.clone(), m)).collect();
            let Inner {
                members, listings, ..
            } = inner;
            listings.retain(|user, _| members.contains_key(user));
            missing = members
                .values()
                .filter(|member| !listings.contains_key(&member.id))
                .cloned()
                .collect();
        });
        missing
    }

    /// Records the documents `user`'s index currently lists. Ignored for
    /// users that are not members.
    pub fn set_listing(&self, user: &str, listed: impl IntoIterator<Item = DocumentId>) {
        let listed: HashSet<DocumentId> = listed.into_iter().collect();
        {
            let inner = self.inner.read().unwrap();
            if !inner.members.contains_key(user) || inner.listings.get(user) == Some(&listed) {
                return;
            }
        }
        self.update(|inner| {
            inner.listings.insert(user.to_owned(), listed);
        });
    }

    /// The account whose index document is `doc`.
    #[must_use]
    pub fn index_owner(&self, doc: DocumentId) -> Option<String> {
        let inner = self.inner.read().unwrap();
        inner
            .members
            .values()
            .find(|member| member.index == doc)
            .map(|member| member.id.clone())
    }

    /// Every member account, deleted ones included.
    #[must_use]
    pub fn members(&self) -> Vec<Member> {
        self.inner
            .read()
            .unwrap()
            .members
            .values()
            .cloned()
            .collect()
    }

    // ---------- servers ----------

    /// Replaces the trusted member servers (id to public key).
    pub fn set_servers(&self, servers: HashMap<String, Vec<u8>>) {
        self.inner.write().unwrap().servers = servers;
    }

    /// Trusts `server_id` while a join fetches the root from it.
    pub fn set_joining(&self, server_id: Option<String>) {
        self.inner.write().unwrap().joining = server_id;
    }

    /// Whether a peer presenting `pubkey` may link: a non-revoked member,
    /// or the server a join is fetching from.
    #[must_use]
    pub fn is_trusted_key(&self, pubkey: &[u8]) -> bool {
        let id = server_id(pubkey);
        let inner = self.inner.read().unwrap();
        inner.servers.get(&id).is_some_and(|key| key == pubkey)
            || inner.joining.as_deref() == Some(id.as_str())
    }

    #[must_use]
    pub fn is_trusted_server(&self, id: &str) -> bool {
        let inner = self.inner.read().unwrap();
        inner.servers.contains_key(id) || inner.joining.as_deref() == Some(id)
    }

    /// Whether `doc` is reachable from the registry.
    #[must_use]
    pub fn in_closure(&self, doc: DocumentId) -> bool {
        self.inner.read().unwrap().closure.contains(&doc)
    }

    /// The registry closure, sorted.
    #[must_use]
    pub fn closure(&self) -> Vec<DocumentId> {
        let mut closure: Vec<DocumentId> =
            self.inner.read().unwrap().closure.iter().copied().collect();
        closure.sort();
        closure
    }

    /// Changes whenever the closure does.
    #[must_use]
    pub fn subscribe_closure(&self) -> watch::Receiver<u64> {
        self.closure_version.subscribe()
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

/// [`AccessPolicy`] over the index: a client peer sees exactly the
/// documents its user owns; a trusted server peer sees the registry
/// closure. Peers without either identity see nothing.
pub struct DerivedPolicy(pub Arc<AccessIndex>);

impl AccessPolicy for DerivedPolicy {
    fn may_sync(&self, peer: &PeerId, document: DocumentId) -> bool {
        if let Some(server) = server_of(peer) {
            return self.0.is_trusted_server(server) && self.0.in_closure(document);
        }
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
            active: true,
        }
    }

    #[test]
    fn peer_ids_carry_the_user() {
        let peer = PeerId::from("0b5c-uuid.1f2e3d/tt-device");
        assert_eq!(user_of(&peer), Some("0b5c-uuid"));
        assert_eq!(user_of(&PeerId::from("tt-device")), None);
        assert_eq!(user_of(&PeerId::from("no-nonce/tt-device")), None);
        assert_eq!(server_of(&server_peer("abc")), Some("abc"));
        assert_eq!(user_of(&server_peer("abc")), None);
    }

    #[test]
    fn a_client_sending_a_server_formatted_id_stays_its_user() {
        // The session identity is prepended to whatever senderId the client
        // sends, so `srv:` in the sender id never makes a server peer.
        let peer = PeerId::from("0b5c-uuid.1f2e3d/srv:aaaabbbbccccddddeeeeffffgg");
        assert_eq!(server_of(&peer), None);
        assert_eq!(user_of(&peer), Some("0b5c-uuid"));
        let index = AccessIndex::new();
        let alice = member("0b5c-uuid", 1);
        let bob = member("bob", 2);
        index.set_members(vec![alice.clone(), bob.clone()]);
        index.set_servers(HashMap::from([(
            "aaaabbbbccccddddeeeeffffgg".into(),
            vec![1; 32],
        )]));
        let policy = DerivedPolicy(index.clone());
        assert!(policy.may_sync(&peer, alice.index));
        assert!(
            !policy.may_sync(&peer, bob.index),
            "only its own user's documents"
        );
        assert!(
            policy.may_sync(&server_peer("aaaabbbbccccddddeeeeffffgg"), bob.index),
            "a real server peer of the same id sees the closure"
        );
        // Also with a sender id that looks like a whole server peer id.
        assert_eq!(server_of(&PeerId::from("srv:x/y")), None);
    }

    #[test]
    fn trusted_servers_sync_the_closure_and_nothing_else() {
        let index = AccessIndex::new();
        let registry = DocumentId::new();
        index.set_registry(Some(registry));
        let mut versions = index.subscribe_closure();
        let (alice, mut gone) = (member("alice", 1), member("gone", 2));
        gone.active = false;
        index.set_members(vec![alice.clone(), gone.clone()]);
        let (workspace, archived) = (DocumentId::new(), DocumentId::new());
        index.set_listing("alice", [workspace]);
        index.set_listing("gone", [archived]);
        assert!(versions.has_changed().unwrap());
        versions.mark_unchanged();
        let key = vec![5_u8; 32];
        let id = server_id(&key);
        index.set_servers(HashMap::from([(id.clone(), key.clone())]));
        let policy = DerivedPolicy(index.clone());
        let peer = server_peer(&id);
        for doc in [registry, alice.index, workspace, gone.index, archived] {
            assert!(policy.may_sync(&peer, doc), "closure document");
        }
        assert!(
            !policy.may_sync(&peer, DocumentId::new()),
            "outside the closure"
        );
        assert!(
            !policy.may_sync(&server_peer("stranger"), workspace),
            "untrusted"
        );
        assert!(index.is_trusted_key(&key));
        assert!(!index.is_trusted_key(&[6; 32]));
        // A deleted account owns nothing for clients.
        assert_eq!(index.access(archived, "gone"), Access::Unlisted);
        // Re-recording the same listing does not bump the closure.
        index.set_listing("alice", [workspace]);
        assert!(!versions.has_changed().unwrap());
        // The join source is trusted by id only while joining.
        let source = server_id(&[7; 32]);
        index.set_joining(Some(source.clone()));
        assert!(index.is_trusted_key(&[7; 32]));
        assert!(policy.may_sync(&server_peer(&source), registry));
        index.set_joining(None);
        assert!(!index.is_trusted_key(&[7; 32]));
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
