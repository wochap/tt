//! Account administration on the server host (`tt-server user|token`).
//! Works on the database file directly, with or without a running server.

use std::path::Path;

use anyhow::{Context, Result, bail};
use automerge::{Automerge, transaction::CommitOptions};
use automerge_repo::{DocumentId, SqliteStorage, storage::StorageAdapter};
use tt_core::{model::User, schema};

use crate::{
    auth::{MIN_PASSWORD_LEN, hash_password},
    db::{Db, TokenRecord, UserRecord, now_ms},
};

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("user {0:?} already exists")]
    Duplicate(String),
    #[error("no user named {0:?}")]
    NoSuchUser(String),
    #[error("no token matches {0:?}")]
    NoSuchToken(String),
    #[error("{0:?} matches several tokens; give more characters")]
    AmbiguousToken(String),
    #[error("{0}")]
    Invalid(String),
}

fn check_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AdminError::Invalid(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        ))
        .into());
    }
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !valid {
        return Err(AdminError::Invalid(
            "user names are 1-64 characters of a-z, A-Z, 0-9, '-', '_', '.'".into(),
        )
        .into());
    }
    Ok(())
}

/// A fresh document with one empty commit plus `init`, as `Repo::create_with`
/// makes them.
fn new_document(
    init: impl FnOnce(
        &mut automerge::transaction::Transaction<'_>,
    ) -> Result<(), automerge::AutomergeError>,
) -> Result<Vec<u8>> {
    let mut doc = Automerge::new();
    doc.empty_commit(CommitOptions::default());
    doc.transact(|tx| init(tx))
        .map_err(|failure| anyhow::anyhow!("initializing document: {}", failure.error))?;
    Ok(doc.save())
}

/// Creates the user's index and workspace documents in the server store and
/// the account with owner ACL rows. Fails with [`AdminError::Duplicate`]
/// (and changes nothing) when the name exists.
pub async fn add_user(db_path: &Path, name: &str, password: &str) -> Result<UserRecord> {
    check_name(name)?;
    check_password(password)?;
    let db = Db::open(db_path)?;
    if db.user_by_name(name)?.is_some() {
        return Err(AdminError::Duplicate(name.into()).into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let user = User {
        id: id.clone(),
        name: name.to_owned(),
    };
    let workspace_id = DocumentId::new();
    let index_id = DocumentId::new();
    let workspace = new_document(|tx| schema::init_workspace(tx, &user))?;
    let workspace_text = workspace_id.to_bs58check();
    let index = new_document(|tx| schema::init_index(tx, &workspace_text))?;
    let password_hash = hash_password(password)?;
    let storage = SqliteStorage::open(db_path)?;
    storage.store(workspace_id, workspace).await?;
    storage.store(index_id, index).await?;
    storage.flush().await?;
    let record = UserRecord {
        id,
        name: name.to_owned(),
        password_hash,
        index_doc: index_id.to_bs58check(),
        workspace_doc: workspace_text,
        created: now_ms(),
    };
    if let Err(error) = db.insert_user(&record) {
        // Lost a race for the name: drop the documents made for it.
        let _ = storage.remove(workspace_id).await;
        let _ = storage.remove(index_id).await;
        let _ = storage.flush().await;
        if db.user_by_name(name)?.is_some() {
            return Err(AdminError::Duplicate(name.into()).into());
        }
        return Err(error);
    }
    Ok(record)
}

pub fn set_password(db_path: &Path, name: &str, password: &str) -> Result<()> {
    check_password(password)?;
    let db = Db::open(db_path)?;
    let user = db
        .user_by_name(name)?
        .ok_or_else(|| AdminError::NoSuchUser(name.into()))?;
    db.set_password(&user.id, &hash_password(password)?)?;
    Ok(())
}

pub fn list_users(db_path: &Path) -> Result<Vec<UserRecord>> {
    Db::open(db_path)?.users()
}

pub fn list_tokens(db_path: &Path) -> Result<Vec<TokenRecord>> {
    Db::open(db_path)?.tokens()
}

/// Revokes the live token whose id (hash prefix, at least 6 characters)
/// matches. A running server closes its websockets within its poll interval.
pub fn revoke_token(db_path: &Path, id: &str) -> Result<TokenRecord> {
    if id.len() < 6 {
        bail!(AdminError::Invalid(
            "give at least 6 characters of the token id".into()
        ));
    }
    let db = Db::open(db_path)?;
    let matches: Vec<TokenRecord> = db
        .tokens()?
        .into_iter()
        .filter(|token| token.revoked.is_none() && token.token_hash.starts_with(id))
        .collect();
    let token = match matches.as_slice() {
        [] => return Err(AdminError::NoSuchToken(id.into()).into()),
        [one] => one.clone(),
        _ => return Err(AdminError::AmbiguousToken(id.into()).into()),
    };
    db.revoke_token(&token.token_hash)
        .context("revoking token")?;
    Ok(token)
}

/// Revokes every live token of a user; returns how many.
pub fn revoke_user_tokens(db_path: &Path, name: &str) -> Result<usize> {
    let db = Db::open(db_path)?;
    let user = db
        .user_by_name(name)?
        .ok_or_else(|| AdminError::NoSuchUser(name.into()))?;
    let mut count = 0;
    for token in db.tokens()? {
        if token.user_id == user.id
            && token.revoked.is_none()
            && db.revoke_token(&token.token_hash)?
        {
            count += 1;
        }
    }
    Ok(count)
}
