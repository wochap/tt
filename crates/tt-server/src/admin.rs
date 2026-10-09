//! Administration on the server host (`tt-server init|user|token`).
//!
//! `serve` holds an exclusive `flock` on `server.db.lock`. A command first
//! tries to take that lock: when it gets it, no server is running and the
//! command opens the database directly (the registry through a local
//! repository on the same storage). When the lock is held, the command
//! calls the running server over its admin socket instead, so no registry
//! write of the server is lost and the change takes effect at once. While
//! another direct command holds the lock (no socket answers), the command
//! waits for it. Passwords are hashed here, before anything is sent.

use std::{
    fs::File,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::{
    admin_socket::{self, WireError},
    app::{App, ServerOptions, UserRef},
    auth::{MIN_PASSWORD_LEN, hash_password},
    db::{RootRecord, TokenRecord},
    registry::Account,
};

/// How long a command waits for a lock held by another direct command.
const LOCK_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("user {0:?} already exists")]
    Duplicate(String),
    #[error("no user {0:?}")]
    NoSuchUser(String),
    #[error("no token matches {0:?}")]
    NoSuchToken(String),
    #[error("{0:?} matches several tokens; give more characters")]
    AmbiguousToken(String),
    #[error("{0}")]
    Invalid(String),
    #[error("this server already has a root; nothing changed")]
    AlreadyInitialized,
    #[error("{}", crate::app::NOT_SET_UP)]
    NotSetUp,
}

impl AdminError {
    /// Process exit code: 4 for conflicts, 3 without a root, 2 for unknown
    /// names/ids and invalid input.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Duplicate(_) | Self::AlreadyInitialized => 4,
            Self::NotSetUp => 3,
            _ => 2,
        }
    }
}

pub(crate) fn check_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AdminError::Invalid(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        ))
        .into());
    }
    Ok(())
}

pub(crate) fn check_name(name: &str) -> Result<()> {
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

/// `server.db.lock` next to the database.
#[must_use]
pub fn lock_path(db: &Path) -> PathBuf {
    let mut name = db.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    db.with_file_name(name)
}

/// The exclusive lock on a database, released on drop.
#[derive(Debug)]
pub struct DbLock {
    _file: File,
}

impl DbLock {
    /// Takes the lock without waiting; `None` when another process (or
    /// another open in this process) holds it.
    pub fn try_acquire(db: &Path) -> Result<Option<Self>> {
        let path = lock_path(db);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => {
                Err(error).with_context(|| format!("locking {}", path.display()))
            }
        }
    }
}

/// Runs one admin method through the running server, or directly.
async fn call(db: &Path, method: &str, params: Value) -> Result<Value> {
    let socket = admin_socket::socket_path(db);
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        if let Some(lock) = DbLock::try_acquire(db)? {
            let app = App::open(ServerOptions::new(db)).await?;
            let result = admin_socket::dispatch(&app, method, params).await;
            let closed = app.close().await;
            drop(lock);
            let value = result?;
            closed?;
            return Ok(value);
        }
        if let Ok(stream) = UnixStream::connect(&socket).await {
            return request(stream, method, params).await;
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "{} is locked by another tt-server process that does not answer on {}",
                db.display(),
                socket.display()
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn request(stream: UnixStream, method: &str, params: Value) -> Result<Value> {
    let (read, mut write) = stream.into_split();
    let mut line = serde_json::to_string(&json!({
        "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
    }))?;
    line.push('\n');
    write.write_all(line.as_bytes()).await?;
    write.flush().await?;
    let mut reply = String::new();
    BufReader::new(read)
        .read_line(&mut reply)
        .await
        .context("reading the admin socket")?;
    let mut reply: Value =
        serde_json::from_str(&reply).context("the server closed the admin socket")?;
    if let Some(error) = reply.get("error").filter(|error| !error.is_null()) {
        let error: WireError = serde_json::from_value(error.clone())?;
        return Err(error.into_error());
    }
    Ok(reply["result"].take())
}

async fn typed<T: DeserializeOwned>(db: &Path, method: &str, params: Value) -> Result<T> {
    Ok(serde_json::from_value(call(db, method, params).await?)?)
}

/// Creates the root. Fails with [`AdminError::AlreadyInitialized`] when
/// there is one.
pub async fn init(db: &Path, name: Option<&str>) -> Result<RootRecord> {
    typed(db, "init", json!({"name": name})).await
}

/// Creates the account and its index and workspace documents. Fails with
/// [`AdminError::Duplicate`] (and changes nothing) when the name exists.
pub async fn add_user(db: &Path, name: &str, password: &str) -> Result<Account> {
    check_name(name)?;
    check_password(password)?;
    let hash = hash_password(password)?;
    typed(db, "user.add", json!({"name": name, "password_hash": hash})).await
}

pub async fn set_password(db: &Path, user: &UserRef, password: &str) -> Result<Account> {
    check_password(password)?;
    let hash = hash_password(password)?;
    typed(
        db,
        "user.passwd",
        json!({"user": user, "password_hash": hash}),
    )
    .await
}

pub async fn rename_user(db: &Path, user: &UserRef, new_name: &str) -> Result<Account> {
    check_name(new_name)?;
    typed(
        db,
        "user.rename",
        json!({"user": user, "new_name": new_name}),
    )
    .await
}

/// Tombstones the account; its tokens are revoked and documents kept.
pub async fn delete_user(db: &Path, user: &UserRef) -> Result<Account> {
    typed(db, "user.del", json!({"user": user})).await
}

pub async fn list_users(db: &Path) -> Result<Vec<Account>> {
    typed(db, "user.ls", json!({})).await
}

pub async fn list_tokens(db: &Path) -> Result<Vec<TokenRecord>> {
    typed(db, "token.ls", json!({})).await
}

/// Revokes the live token whose id (hash prefix, at least 6 characters)
/// matches. A running server closes its websockets at once.
pub async fn revoke_token(db: &Path, id: &str) -> Result<TokenRecord> {
    typed(db, "token.revoke", json!({"id": id})).await
}

/// Revokes every live token of a user; returns how many.
pub async fn revoke_user_tokens(db: &Path, name: &str) -> Result<usize> {
    typed(
        db,
        "token.revoke",
        json!({"user": UserRef::Name(name.into())}),
    )
    .await
}
