//! The registry document: the server's root, holding every account.
//!
//! ```text
//! { kind: "tt-registry", version: 1,
//!   users: { <uuid>: {
//!       index_doc, workspace_doc, created,   // written once
//!       name:     { value, at },             // versioned field
//!       password: { hash, at },              // versioned field
//!       deleted:  <ms> | absent } } }
//! ```
//!
//! Merges must resolve the same way on every server whatever order changes
//! arrive in, so readers never trust Automerge's choice among conflicting
//! values: a versioned field is a whole map written under one key, and the
//! reader takes every conflicting value (`get_all`) and keeps the greatest
//! `(at, value)`. `deleted` is final: any value present means deleted.
//! Nothing ever removes a key. Strings are scalars, never `Text`.

use std::collections::BTreeMap;

use automerge::{
    AutomergeError, ObjId, ObjType, ROOT, ReadDoc, ScalarValue, Value, transaction::Transactable,
};
use serde::{Deserialize, Serialize};
use tt_core::am::{get_i64, get_map, get_string};

pub const KIND_REGISTRY: &str = "tt-registry";
pub const VERSION: i64 = 1;

type AmResult<T> = Result<T, AutomergeError>;

/// Writes the empty registry.
pub fn init<T: Transactable + ReadDoc>(tx: &mut T) -> AmResult<()> {
    tx.put(ROOT, "kind", KIND_REGISTRY)?;
    tx.put(ROOT, "version", VERSION)?;
    tx.put_object(ROOT, "users", ObjType::Map)?;
    Ok(())
}

/// Whether `doc` is a registry document.
pub fn is_registry<D: ReadDoc>(doc: &D) -> bool {
    get_string(doc, &ROOT, "kind").as_deref() == Some(KIND_REGISTRY)
}

/// A new account's write-once fields and first versioned values.
#[derive(Clone, Debug)]
pub struct NewAccount {
    pub id: String,
    pub name: String,
    pub index_doc: String,
    pub workspace_doc: String,
    pub password_hash: String,
    pub created: i64,
}

fn users<T: Transactable + ReadDoc>(tx: &mut T) -> AmResult<ObjId> {
    match get_map(tx, &ROOT, "users") {
        Some(users) => Ok(users),
        None => tx.put_object(ROOT, "users", ObjType::Map),
    }
}

fn account<T: Transactable + ReadDoc>(tx: &mut T, id: &str) -> AmResult<ObjId> {
    let users = users(tx)?;
    get_map(tx, &users, id).ok_or_else(|| AutomergeError::InvalidObjId(format!("no account {id}")))
}

fn put_versioned<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
    value_key: &str,
    value: &str,
    at: i64,
) -> AmResult<()> {
    let field = tx.put_object(obj, key, ObjType::Map)?;
    tx.put(&field, value_key, value)?;
    tx.put(&field, "at", at)?;
    Ok(())
}

pub fn add_account<T: Transactable + ReadDoc>(tx: &mut T, account: &NewAccount) -> AmResult<()> {
    let users = users(tx)?;
    let obj = tx.put_object(&users, account.id.as_str(), ObjType::Map)?;
    tx.put(&obj, "index_doc", account.index_doc.as_str())?;
    tx.put(&obj, "workspace_doc", account.workspace_doc.as_str())?;
    tx.put(&obj, "created", account.created)?;
    put_versioned(tx, &obj, "name", "value", &account.name, account.created)?;
    put_versioned(
        tx,
        &obj,
        "password",
        "hash",
        &account.password_hash,
        account.created,
    )
}

pub fn set_password<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: &str,
    hash: &str,
    at: i64,
) -> AmResult<()> {
    let obj = account(tx, id)?;
    put_versioned(tx, &obj, "password", "hash", hash, at)
}

pub fn set_name<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: &str,
    name: &str,
    at: i64,
) -> AmResult<()> {
    let obj = account(tx, id)?;
    put_versioned(tx, &obj, "name", "value", name, at)
}

/// Tombstones an account. A deleted account stays deleted.
pub fn delete_account<T: Transactable + ReadDoc>(tx: &mut T, id: &str, at: i64) -> AmResult<()> {
    let obj = account(tx, id)?;
    if tx.get(&obj, "deleted")?.is_none() {
        tx.put(&obj, "deleted", at)?;
    }
    Ok(())
}

/// One account as every server resolves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub name_changed_at: i64,
    pub index_doc: String,
    pub workspace_doc: String,
    pub created: i64,
    pub password_hash: String,
    pub password_changed_at: i64,
    pub deleted: Option<i64>,
    /// Another non-deleted account with the same name was created first;
    /// this one cannot log in until renamed.
    pub conflicted: bool,
}

impl Account {
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        self.deleted.is_some()
    }
}

/// Every conflicting value of a versioned field, resolved to the greatest
/// `(at, value)`.
fn versioned<D: ReadDoc>(
    doc: &D,
    obj: &ObjId,
    key: &str,
    value_key: &str,
) -> Option<(String, i64)> {
    doc.get_all(obj, key)
        .ok()?
        .into_iter()
        .filter_map(|(value, id)| match value {
            Value::Object(ObjType::Map) => {
                Some((get_i64(doc, &id, "at")?, get_string(doc, &id, value_key)?))
            }
            _ => None,
        })
        .max()
        .map(|(at, value)| (value, at))
}

fn deleted<D: ReadDoc>(doc: &D, obj: &ObjId) -> Option<i64> {
    doc.get_all(obj, "deleted")
        .ok()?
        .into_iter()
        .filter_map(|(value, _)| match value {
            Value::Scalar(scalar) => match scalar.as_ref() {
                ScalarValue::Int(at) | ScalarValue::Timestamp(at) => Some(*at),
                ScalarValue::Uint(at) => i64::try_from(*at).ok(),
                _ => Some(0),
            },
            _ => Some(0),
        })
        .min()
}

/// Accounts by id, resolved and with conflict flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryView {
    accounts: BTreeMap<String, Account>,
}

/// Reads the registry. Accounts missing write-once fields (a partial merge
/// that cannot happen with this writer) are skipped.
pub fn read<D: ReadDoc>(doc: &D) -> RegistryView {
    let mut accounts = BTreeMap::new();
    if let Some(users) = get_map(doc, &ROOT, "users") {
        for id in doc.keys(&users).collect::<Vec<_>>() {
            let Some(obj) = get_map(doc, &users, &id) else {
                continue;
            };
            let read = || {
                let (name, name_changed_at) = versioned(doc, &obj, "name", "value")?;
                let (password_hash, password_changed_at) =
                    versioned(doc, &obj, "password", "hash")?;
                Some(Account {
                    id: id.clone(),
                    name,
                    name_changed_at,
                    index_doc: get_string(doc, &obj, "index_doc")?,
                    workspace_doc: get_string(doc, &obj, "workspace_doc")?,
                    created: get_i64(doc, &obj, "created")?,
                    password_hash,
                    password_changed_at,
                    deleted: deleted(doc, &obj),
                    conflicted: false,
                })
            };
            if let Some(account) = read() {
                accounts.insert(id, account);
            }
        }
    }
    let mut view = RegistryView { accounts };
    view.flag_conflicts();
    view
}

impl RegistryView {
    /// Among non-deleted accounts sharing a name, the smallest `(created,
    /// id)` owns it; the rest are conflicted.
    fn flag_conflicts(&mut self) {
        let mut owners: BTreeMap<&str, (i64, &str)> = BTreeMap::new();
        for account in self.accounts.values().filter(|a| !a.is_deleted()) {
            let key = (account.created, account.id.as_str());
            owners
                .entry(account.name.as_str())
                .and_modify(|owner| *owner = (*owner).min(key))
                .or_insert(key);
        }
        let owners: BTreeMap<String, String> = owners
            .into_iter()
            .map(|(name, (_, id))| (name.to_owned(), id.to_owned()))
            .collect();
        for account in self.accounts.values_mut() {
            account.conflicted =
                !account.is_deleted() && owners.get(&account.name) != Some(&account.id);
        }
    }

    /// An account by id, deleted or not.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.get(id)
    }

    /// A non-deleted account by id.
    #[must_use]
    pub fn active(&self, id: &str) -> Option<&Account> {
        self.get(id).filter(|account| !account.is_deleted())
    }

    /// Non-deleted accounts named `name`, the owner first.
    #[must_use]
    pub fn named(&self, name: &str) -> Vec<&Account> {
        let mut named: Vec<&Account> = self
            .accounts
            .values()
            .filter(|a| !a.is_deleted() && a.name == name)
            .collect();
        named.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        named
    }

    /// The account owning `name`.
    #[must_use]
    pub fn owner(&self, name: &str) -> Option<&Account> {
        self.named(name).into_iter().next()
    }

    /// Every account, deleted ones included, by name then creation.
    #[must_use]
    pub fn accounts(&self) -> Vec<&Account> {
        let mut all: Vec<&Account> = self.accounts.values().collect();
        all.sort_by(|a, b| (&a.name, a.created, &a.id).cmp(&(&b.name, b.created, &b.id)));
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::AutoCommit;

    fn new_account(id: &str, name: &str, created: i64) -> NewAccount {
        NewAccount {
            id: id.into(),
            name: name.into(),
            index_doc: format!("index-{id}"),
            workspace_doc: format!("workspace-{id}"),
            password_hash: format!("hash-{id}"),
            created,
        }
    }

    /// A registry with bob, forked into two servers' copies.
    fn forked() -> (AutoCommit, AutoCommit) {
        let mut a = AutoCommit::new();
        init(&mut a).unwrap();
        add_account(&mut a, &new_account("b0b", "bob", 100)).unwrap();
        let b = a.fork();
        (a, b)
    }

    /// Merges both ways and checks both sides read the same view.
    fn merge_both(mut a: AutoCommit, mut b: AutoCommit) -> RegistryView {
        let mut left = a.fork();
        left.merge(&mut b).unwrap();
        let mut right = b.fork();
        right.merge(&mut a).unwrap();
        let (left, right) = (read(&left), read(&right));
        assert_eq!(left, right, "merge order must not matter");
        left
    }

    #[test]
    fn accounts_round_trip() {
        let mut doc = AutoCommit::new();
        init(&mut doc).unwrap();
        assert!(is_registry(&doc));
        add_account(&mut doc, &new_account("a1", "alice", 5)).unwrap();
        let view = read(&doc);
        let alice = view.owner("alice").unwrap();
        assert_eq!(alice.id, "a1");
        assert_eq!(alice.index_doc, "index-a1");
        assert_eq!(alice.password_hash, "hash-a1");
        assert_eq!((alice.password_changed_at, alice.deleted), (5, None));
        assert!(!alice.conflicted);
    }

    #[test]
    fn concurrent_password_changes_keep_the_latest() {
        let (mut a, mut b) = forked();
        set_password(&mut a, "b0b", "hash-at-10-00", 1000).unwrap();
        set_password(&mut b, "b0b", "hash-at-10-05", 1005).unwrap();
        let view = merge_both(a, b);
        assert_eq!(view.get("b0b").unwrap().password_hash, "hash-at-10-05");

        // Same time: the greater hash wins, on both sides.
        let (mut a, mut b) = forked();
        set_password(&mut a, "b0b", "hash-x", 1000).unwrap();
        set_password(&mut b, "b0b", "hash-y", 1000).unwrap();
        assert_eq!(merge_both(a, b).get("b0b").unwrap().password_hash, "hash-y");
    }

    #[test]
    fn delete_wins_against_a_password_change() {
        let (mut a, mut b) = forked();
        delete_account(&mut a, "b0b", 1000).unwrap();
        set_password(&mut b, "b0b", "new-hash", 2000).unwrap();
        let view = merge_both(a, b);
        let bob = view.get("b0b").unwrap();
        assert_eq!(bob.deleted, Some(1000));
        assert!(view.active("b0b").is_none());
        assert!(view.owner("bob").is_none());

        // Deleted twice concurrently: still deleted, at the earlier time.
        let (mut a, mut b) = forked();
        delete_account(&mut a, "b0b", 1000).unwrap();
        delete_account(&mut b, "b0b", 900).unwrap();
        assert_eq!(merge_both(a, b).get("b0b").unwrap().deleted, Some(900));
    }

    #[test]
    fn concurrent_renames_keep_the_latest_then_the_greater_name() {
        let (mut a, mut b) = forked();
        set_name(&mut a, "b0b", "robert", 1005).unwrap();
        set_name(&mut b, "b0b", "bobby", 1000).unwrap();
        assert_eq!(merge_both(a, b).get("b0b").unwrap().name, "robert");

        let (mut a, mut b) = forked();
        set_name(&mut a, "b0b", "alpha", 1000).unwrap();
        set_name(&mut b, "b0b", "omega", 1000).unwrap();
        let view = merge_both(a, b);
        let bob = view.get("b0b").unwrap();
        assert_eq!((bob.name.as_str(), bob.name_changed_at), ("omega", 1000));
    }

    #[test]
    fn duplicate_names_from_two_forks_flag_the_later_account() {
        let mut origin = AutoCommit::new();
        init(&mut origin).unwrap();
        let (mut a, mut b) = (origin.fork(), origin.fork());
        add_account(&mut a, &new_account("id-2", "bob", 200)).unwrap();
        add_account(&mut b, &new_account("id-1", "bob", 300)).unwrap();
        let view = merge_both(a, b);
        assert_eq!(
            view.owner("bob").unwrap().id,
            "id-2",
            "earlier created owns"
        );
        assert!(view.get("id-1").unwrap().conflicted);
        assert!(!view.get("id-2").unwrap().conflicted);

        // Same creation time: the lower id owns.
        let (mut a, mut b) = (origin.fork(), origin.fork());
        add_account(&mut a, &new_account("id-b", "bob", 200)).unwrap();
        add_account(&mut b, &new_account("id-a", "bob", 200)).unwrap();
        let view = merge_both(a.fork(), b.fork());
        assert_eq!(view.owner("bob").unwrap().id, "id-a");

        // Renaming the conflicted account clears the flag.
        let mut merged = a.fork();
        merged.merge(&mut b).unwrap();
        set_name(&mut merged, "id-b", "bob2", 300).unwrap();
        let view = read(&merged);
        assert!(view.accounts().iter().all(|account| !account.conflicted));
        assert_eq!(view.owner("bob2").unwrap().id, "id-b");

        // Deleting the owner hands the name to the other account.
        let mut merged = a.fork();
        merged.merge(&mut b).unwrap();
        delete_account(&mut merged, "id-a", 300).unwrap();
        let view = read(&merged);
        assert_eq!(view.owner("bob").unwrap().id, "id-b");
        assert!(!view.get("id-b").unwrap().conflicted);
    }
}
