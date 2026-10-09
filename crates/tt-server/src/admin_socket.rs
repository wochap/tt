//! `admin.sock`: newline-delimited JSON-RPC 2.0 for the admin commands,
//! served by `serve` in the database directory (mode 0600: the file
//! permissions are the credential). Methods: `init`, `user.add`,
//! `user.passwd`, `user.rename`, `user.del`, `user.ls`, `token.ls`,
//! `token.revoke`. Password hashes arrive already hashed; the socket never
//! carries a plaintext password. [`dispatch`] is also what direct admin
//! commands run, so both paths share one implementation.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use tracing::{debug, warn};

use crate::{
    admin::AdminError,
    app::{App, UserRef},
};

const INVALID_PARAMS: i64 = -32602;
const INTERNAL: i64 = -32603;
const NOT_FOUND: i64 = -32002;
const UNAVAILABLE: i64 = -32003;
const CONFLICT: i64 = -32004;

/// `admin.sock` in the database directory.
#[must_use]
pub fn socket_path(db: &Path) -> PathBuf {
    db.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("admin.sock")
}

/// Binds the socket, replacing a stale file (the caller holds the database
/// lock, so no live server owns it).
pub(crate) fn bind(path: &Path) -> Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting {}", path.display()))?;
    }
    Ok(listener)
}

pub(crate) async fn serve(app: Arc<App>, listener: UnixListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(app.clone(), stream));
            }
            Err(error) => {
                warn!(%error, "admin socket accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

/// An error as it travels over the socket; `kind` rebuilds the
/// [`AdminError`] on the client so exit codes match the direct path.
#[derive(Debug, Serialize, Deserialize)]
pub struct WireError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Value,
}

impl WireError {
    fn from_error(error: &anyhow::Error) -> Self {
        let Some(admin) = error.downcast_ref::<AdminError>() else {
            return Self {
                code: INTERNAL,
                message: format!("{error:#}"),
                data: Value::Null,
            };
        };
        let (code, kind, value) = match admin {
            AdminError::Duplicate(name) => (CONFLICT, "duplicate", name.clone()),
            AdminError::AlreadyInitialized => (CONFLICT, "already_initialized", String::new()),
            AdminError::NotSetUp => (UNAVAILABLE, "not_set_up", String::new()),
            AdminError::NoSuchUser(name) => (NOT_FOUND, "no_such_user", name.clone()),
            AdminError::NoSuchToken(id) => (NOT_FOUND, "no_such_token", id.clone()),
            AdminError::AmbiguousToken(id) => (INVALID_PARAMS, "ambiguous_token", id.clone()),
            AdminError::Invalid(message) => (INVALID_PARAMS, "invalid", message.clone()),
        };
        Self {
            code,
            message: admin.to_string(),
            data: json!({"kind": kind, "value": value}),
        }
    }

    #[must_use]
    pub fn into_error(self) -> anyhow::Error {
        let value = self.data["value"].as_str().unwrap_or_default().to_owned();
        let admin = match self.data["kind"].as_str() {
            Some("duplicate") => AdminError::Duplicate(value),
            Some("already_initialized") => AdminError::AlreadyInitialized,
            Some("not_set_up") => AdminError::NotSetUp,
            Some("no_such_user") => AdminError::NoSuchUser(value),
            Some("no_such_token") => AdminError::NoSuchToken(value),
            Some("ambiguous_token") => AdminError::AmbiguousToken(value),
            Some("invalid") => AdminError::Invalid(value),
            _ => return anyhow::anyhow!(self.message),
        };
        admin.into()
    }
}

async fn connection(app: Arc<App>, stream: UnixStream) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                debug!(method = %request.method, "admin request");
                match dispatch(&app, &request.method, request.params).await {
                    Ok(result) => json!({"jsonrpc": "2.0", "id": request.id, "result": result}),
                    Err(error) => json!({
                        "jsonrpc": "2.0", "id": request.id,
                        "error": WireError::from_error(&error),
                    }),
                }
            }
            Err(error) => json!({
                "jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": error.to_string()},
            }),
        };
        let mut text = reply.to_string();
        text.push('\n');
        if write.write_all(text.as_bytes()).await.is_err() {
            return;
        }
    }
}

fn params<T: DeserializeOwned>(params: Value) -> Result<T> {
    serde_json::from_value(params)
        .map_err(|error| AdminError::Invalid(format!("invalid parameters: {error}")).into())
}

#[derive(Deserialize)]
struct InitParams {
    name: Option<String>,
}
#[derive(Deserialize)]
struct AddParams {
    name: String,
    password_hash: String,
}
#[derive(Deserialize)]
struct PasswdParams {
    user: UserRef,
    password_hash: String,
}
#[derive(Deserialize)]
struct RenameParams {
    user: UserRef,
    new_name: String,
}
#[derive(Deserialize)]
struct UserParams {
    user: UserRef,
}
#[derive(Deserialize)]
struct RevokeParams {
    id: Option<String>,
    user: Option<UserRef>,
}

/// Runs one admin method against `app`.
pub(crate) async fn dispatch(app: &Arc<App>, method: &str, raw: Value) -> Result<Value> {
    let value = match method {
        "init" => {
            let p: InitParams = params(raw)?;
            serde_json::to_value(app.init(p.name).await?)?
        }
        "user.add" => {
            let p: AddParams = params(raw)?;
            serde_json::to_value(app.add_user(&p.name, &p.password_hash).await?)?
        }
        "user.passwd" => {
            let p: PasswdParams = params(raw)?;
            serde_json::to_value(app.set_password(&p.user, &p.password_hash).await?)?
        }
        "user.rename" => {
            let p: RenameParams = params(raw)?;
            serde_json::to_value(app.rename_user(&p.user, &p.new_name).await?)?
        }
        "user.del" => {
            let p: UserParams = params(raw)?;
            serde_json::to_value(app.delete_user(&p.user).await?)?
        }
        "user.ls" => serde_json::to_value(app.users()?)?,
        "token.ls" => serde_json::to_value(app.tokens().await?)?,
        "token.revoke" => {
            let p: RevokeParams = params(raw)?;
            match (p.id, p.user) {
                (Some(id), None) => serde_json::to_value(app.revoke_token(&id).await?)?,
                (None, Some(user)) => json!(app.revoke_user_tokens(&user).await?),
                _ => {
                    return Err(
                        AdminError::Invalid("give a token id or a user, not both".into()).into(),
                    );
                }
            }
        }
        other => return Err(anyhow::anyhow!("unknown method {other:?}")),
    };
    Ok(value)
}
