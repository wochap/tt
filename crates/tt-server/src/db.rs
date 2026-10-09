//! `server.db`: the root record, tokens, tickets, and the login audit log,
//! next to the `documents`/`control` tables owned by `SqliteStorage` in the
//! same file. Accounts live in the registry document, not here; nothing in
//! these tables is ever replicated.
//!
//! Every record is keyed by text: user ids are UUIDs, documents are bs58check
//! ids, token and ticket secrets are only ever stored as SHA-256 hex. Times
//! are Unix milliseconds.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// `PRAGMA user_version` of the current schema. Version 1 (accounts and ACL
/// rows in SQLite) is refused, never migrated.
pub const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = "
    CREATE TABLE server (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        server_id TEXT NOT NULL,
        name TEXT NOT NULL,
        registry_doc TEXT NOT NULL,
        created INTEGER NOT NULL
    );
    CREATE TABLE tokens (
        token_hash TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        created INTEGER NOT NULL,
        last_used INTEGER,
        revoked INTEGER
    );
    CREATE INDEX tokens_user ON tokens(user_id);
    CREATE TABLE tickets (
        ticket_hash TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        token_hash TEXT NOT NULL REFERENCES tokens(token_hash),
        expires INTEGER NOT NULL,
        used INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE login_audit (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        at INTEGER NOT NULL,
        ip TEXT NOT NULL,
        username TEXT NOT NULL,
        success INTEGER NOT NULL,
        reason TEXT NOT NULL
    );";

#[must_use]
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The database was written by an incompatible tt-server version.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error(
        "{0} has the database format of an older tt-server (accounts in SQLite), which is no longer supported; move it aside and run `tt-server init`"
    )]
    Old(String),
    #[error("{0} was written by a newer tt-server (schema version {1})")]
    Newer(String, i64),
}

/// The root of this server: the registry document and the server's name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRecord {
    pub server_id: String,
    pub name: String,
    pub registry_doc: String,
    pub created: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    pub token_hash: String,
    pub user_id: String,
    /// Filled from the registry by [`crate::App::tokens`]; empty here.
    pub user_name: String,
    pub created: i64,
    pub last_used: Option<i64>,
    pub revoked: Option<i64>,
}

impl TokenRecord {
    /// Short public id used by `token ls|revoke`: a prefix of the hash.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.token_hash[..12]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    pub at: i64,
    pub ip: String,
    pub username: String,
    pub success: bool,
    pub reason: String,
}

/// Outcome of consuming a websocket ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketGrant {
    pub user_id: String,
    pub token_hash: String,
}

/// Shared connection. Calls from async code go through [`Db::run`], which
/// runs the closure on the blocking pool.
#[derive(Clone)]
pub struct Db {
    connection: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

/// Refuses databases of other schema versions before anything is written.
fn check_format(connection: &Connection, path: &Path) -> Result<bool> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let has_users: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'users')",
        [],
        |row| row.get(0),
    )?;
    let shown = path.display().to_string();
    match version {
        _ if has_users => Err(FormatError::Old(shown).into()),
        0 => Ok(true),
        SCHEMA_VERSION => Ok(false),
        1 => Err(FormatError::Old(shown).into()),
        newer => Err(FormatError::Newer(shown, newer).into()),
    }
}

fn token_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TokenRecord> {
    Ok(TokenRecord {
        token_hash: row.get(0)?,
        user_id: row.get(1)?,
        user_name: String::new(),
        created: row.get(2)?,
        last_used: row.get(3)?,
        revoked: row.get(4)?,
    })
}

impl Db {
    /// Opens (creating if needed) the database. An older format is refused
    /// with [`FormatError`] and the file is left unchanged.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let fresh = check_format(&connection, path)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA foreign_keys=ON;",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("restricting {}", path.display()))?;
        }
        let db = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        if fresh {
            db.with(|connection| {
                let tx = connection.transaction()?;
                tx.execute_batch(SCHEMA)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()
            })?;
        }
        Ok(db)
    }

    /// Runs `job` on the calling thread (CLI and tests).
    pub fn with<T>(&self, job: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut connection = self.connection.lock().unwrap_or_else(|e| e.into_inner());
        Ok(job(&mut connection)?)
    }

    /// Runs `job` on the blocking pool (async callers).
    pub async fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce(&Db) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || job(&db)).await?
    }

    // ---------- root ----------

    /// The root, or `None` while the server is in `NeedsDecision`.
    pub fn root(&self) -> Result<Option<RootRecord>> {
        self.with(|connection| {
            connection
                .query_row(
                    "SELECT server_id, name, registry_doc, created FROM server WHERE id = 1",
                    [],
                    |row| {
                        Ok(RootRecord {
                            server_id: row.get(0)?,
                            name: row.get(1)?,
                            registry_doc: row.get(2)?,
                            created: row.get(3)?,
                        })
                    },
                )
                .optional()
        })
    }

    /// Records the root; `false` (and nothing written) when one exists.
    pub fn insert_root(&self, root: &RootRecord) -> Result<bool> {
        let root = root.clone();
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO server (id, server_id, name, registry_doc, created)
                     VALUES (1, ?1, ?2, ?3, ?4) ON CONFLICT (id) DO NOTHING",
                    params![root.server_id, root.name, root.registry_doc, root.created],
                )
                .map(|changed| changed == 1)
        })
    }

    // ---------- tokens ----------

    pub fn insert_token(&self, token_hash: &str, user_id: &str) -> Result<()> {
        let (token_hash, user_id) = (token_hash.to_owned(), user_id.to_owned());
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO tokens (token_hash, user_id, created) VALUES (?1, ?2, ?3)",
                    params![token_hash, user_id, now_ms()],
                )
                .map(|_| ())
        })
    }

    /// The user id of a live (unrevoked) token; records the use.
    pub fn token_user(&self, token_hash: &str) -> Result<Option<String>> {
        let token_hash = token_hash.to_owned();
        self.with(move |connection| {
            let user = connection
                .query_row(
                    "SELECT user_id FROM tokens WHERE token_hash = ?1 AND revoked IS NULL",
                    params![token_hash],
                    |row| row.get(0),
                )
                .optional()?;
            if user.is_some() {
                connection.execute(
                    "UPDATE tokens SET last_used = ?1 WHERE token_hash = ?2",
                    params![now_ms(), token_hash],
                )?;
            }
            Ok(user)
        })
    }

    /// Revokes one token; `false` when it was unknown or already revoked.
    pub fn revoke_token(&self, token_hash: &str) -> Result<bool> {
        let token_hash = token_hash.to_owned();
        self.with(move |connection| {
            connection
                .execute(
                    "UPDATE tokens SET revoked = ?1 WHERE token_hash = ?2 AND revoked IS NULL",
                    params![now_ms(), token_hash],
                )
                .map(|changed| changed == 1)
        })
    }

    /// Revokes every live token of a user; returns the revoked hashes.
    pub fn revoke_user_tokens(&self, user_id: &str) -> Result<Vec<String>> {
        let user_id = user_id.to_owned();
        self.with(move |connection| {
            let mut statement = connection.prepare(
                "UPDATE tokens SET revoked = ?1 WHERE user_id = ?2 AND revoked IS NULL
                 RETURNING token_hash",
            )?;
            statement
                .query_map(params![now_ms(), user_id], |row| row.get(0))?
                .collect()
        })
    }

    pub fn tokens(&self) -> Result<Vec<TokenRecord>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT token_hash, user_id, created, last_used, revoked
                 FROM tokens ORDER BY created",
            )?;
            statement.query_map([], token_from_row)?.collect()
        })
    }

    /// Of `hashes`, the ones that are no longer live (revoked or unknown).
    pub fn dead_tokens(&self, hashes: Vec<String>) -> Result<Vec<String>> {
        self.with(move |connection| {
            let mut statement = connection
                .prepare("SELECT 1 FROM tokens WHERE token_hash = ?1 AND revoked IS NULL")?;
            let mut dead = Vec::new();
            for hash in hashes {
                if !statement.exists(params![hash])? {
                    dead.push(hash);
                }
            }
            Ok(dead)
        })
    }

    // ---------- tickets ----------

    pub fn insert_ticket(
        &self,
        ticket_hash: &str,
        user_id: &str,
        token_hash: &str,
        expires: i64,
    ) -> Result<()> {
        let (ticket_hash, user_id, token_hash) = (
            ticket_hash.to_owned(),
            user_id.to_owned(),
            token_hash.to_owned(),
        );
        self.with(move |connection| {
            let now = now_ms();
            // Expired and used tickets are useless; keep the table small.
            connection.execute(
                "DELETE FROM tickets WHERE expires < ?1 OR used = 1",
                params![now],
            )?;
            connection
                .execute(
                    "INSERT INTO tickets (ticket_hash, user_id, token_hash, expires)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![ticket_hash, user_id, token_hash, expires],
                )
                .map(|_| ())
        })
    }

    /// Marks a ticket used and returns its grant if it was unused, unexpired,
    /// and its token is still live. A second call for the same ticket fails.
    pub fn consume_ticket(&self, ticket_hash: &str) -> Result<Option<TicketGrant>> {
        let ticket_hash = ticket_hash.to_owned();
        self.with(move |connection| {
            let tx = connection.transaction()?;
            let claimed: Option<(String, String)> = tx
                .query_row(
                    "UPDATE tickets SET used = 1
                     WHERE ticket_hash = ?1 AND used = 0 AND expires > ?2
                     RETURNING user_id, token_hash",
                    params![ticket_hash, now_ms()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let grant = match claimed {
                Some((user_id, token_hash)) => {
                    let live: bool = tx.query_row(
                        "SELECT EXISTS (SELECT 1 FROM tokens WHERE token_hash = ?1 AND revoked IS NULL)",
                        params![token_hash],
                        |row| row.get(0),
                    )?;
                    live.then_some(TicketGrant {
                        user_id,
                        token_hash,
                    })
                }
                None => None,
            };
            tx.commit()?;
            Ok(grant)
        })
    }

    // ---------- audit ----------

    pub fn audit_login(&self, ip: &str, username: &str, success: bool, reason: &str) -> Result<()> {
        let (ip, username, reason) = (ip.to_owned(), username.to_owned(), reason.to_owned());
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO login_audit (at, ip, username, success, reason)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![now_ms(), ip, username, success, reason],
                )
                .map(|_| ())
        })
    }

    pub fn login_audit(&self, limit: usize) -> Result<Vec<AuditRecord>> {
        self.with(move |connection| {
            let mut statement = connection.prepare(
                "SELECT at, ip, username, success, reason FROM login_audit
                 ORDER BY id DESC LIMIT ?1",
            )?;
            statement
                .query_map(params![limit as i64], |row| {
                    Ok(AuditRecord {
                        at: row.get(0)?,
                        ip: row.get(1)?,
                        username: row.get(2)?,
                        success: row.get(3)?,
                        reason: row.get(4)?,
                    })
                })?
                .collect()
        })
    }
}

/// The version 1 schema, for tests that need an old-format database.
#[doc(hidden)]
pub const V1_SCHEMA_FOR_TESTS: &str = "
    CREATE TABLE users (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE,
        password_hash TEXT NOT NULL, index_doc TEXT NOT NULL,
        workspace_doc TEXT NOT NULL, created INTEGER NOT NULL);
    CREATE TABLE acl (doc_id TEXT NOT NULL, user_id TEXT NOT NULL, role TEXT NOT NULL,
        created INTEGER NOT NULL, PRIMARY KEY (doc_id, user_id));
    INSERT INTO users VALUES ('u1', 'alice', 'x', 'i', 'w', 0);
    PRAGMA user_version = 1;";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_gets_the_v2_schema_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(db.root().unwrap(), None);
        let root = RootRecord {
            server_id: "abc".into(),
            name: "laptop-a".into(),
            registry_doc: "doc".into(),
            created: 1,
        };
        assert!(db.insert_root(&root).unwrap());
        assert!(!db.insert_root(&root).unwrap(), "one root only");
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.root().unwrap(), Some(root));
        let (version, tables): (i64, Vec<String>) = db
            .with(|c| {
                let version = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                let mut statement = c.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )?;
                let tables = statement.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                Ok((version, tables))
            })
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(tables, ["login_audit", "server", "tickets", "tokens"]);
    }

    #[test]
    fn version_one_database_is_refused_and_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(V1_SCHEMA_FOR_TESTS)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = Db::open(&path).unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<FormatError>(),
                Some(FormatError::Old(_))
            ),
            "{error:#}"
        );
        assert!(error.to_string().contains("tt-server init"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn revoking_a_user_returns_the_revoked_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        db.insert_token("h1", "alice").unwrap();
        db.insert_token("h2", "alice").unwrap();
        db.insert_token("h3", "bob").unwrap();
        assert!(db.revoke_token("h1").unwrap());
        assert_eq!(db.revoke_user_tokens("alice").unwrap(), ["h2"]);
        assert_eq!(db.token_user("h3").unwrap().as_deref(), Some("bob"));
        assert_eq!(db.token_user("h2").unwrap(), None);
    }
}
