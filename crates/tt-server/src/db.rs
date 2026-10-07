//! `server.db`: accounts, tokens, tickets, ACL and the login audit log, next
//! to the `documents`/`control` tables owned by `SqliteStorage` in the same
//! file.
//!
//! Every record is keyed by text: user ids are UUIDs, documents are bs58check
//! ids (as clients and the index document write them), token and ticket
//! secrets are only ever stored as SHA-256 hex. Times are Unix milliseconds.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use automerge_repo::DocumentId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::acl::Role;

/// Schema versions, applied in order and recorded in `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    // 1: accounts, tokens, tickets, ACL, login audit.
    "CREATE TABLE users (
         id TEXT PRIMARY KEY,
         name TEXT NOT NULL UNIQUE,
         password_hash TEXT NOT NULL,
         index_doc TEXT NOT NULL,
         workspace_doc TEXT NOT NULL,
         created INTEGER NOT NULL
     );
     CREATE TABLE tokens (
         token_hash TEXT PRIMARY KEY,
         user_id TEXT NOT NULL REFERENCES users(id),
         created INTEGER NOT NULL,
         last_used INTEGER,
         revoked INTEGER
     );
     CREATE INDEX tokens_user ON tokens(user_id);
     CREATE TABLE tickets (
         ticket_hash TEXT PRIMARY KEY,
         user_id TEXT NOT NULL REFERENCES users(id),
         token_hash TEXT NOT NULL REFERENCES tokens(token_hash),
         expires INTEGER NOT NULL,
         used INTEGER NOT NULL DEFAULT 0
     );
     CREATE TABLE acl (
         doc_id TEXT NOT NULL,
         user_id TEXT NOT NULL REFERENCES users(id),
         role TEXT NOT NULL CHECK (role IN ('owner', 'writer', 'reader')),
         created INTEGER NOT NULL,
         PRIMARY KEY (doc_id, user_id)
     );
     CREATE INDEX acl_user ON acl(user_id);
     CREATE TABLE login_audit (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         at INTEGER NOT NULL,
         ip TEXT NOT NULL,
         username TEXT NOT NULL,
         success INTEGER NOT NULL,
         reason TEXT NOT NULL
     );",
];

#[must_use]
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserRecord {
    pub id: String,
    pub name: String,
    pub password_hash: String,
    pub index_doc: String,
    pub workspace_doc: String,
    pub created: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenRecord {
    pub token_hash: String,
    pub user_id: String,
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
    pub user: UserRecord,
    pub token_hash: String,
}

/// Shared connection. Calls from async code go through [`Db::call`], which
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

fn user_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRecord> {
    Ok(UserRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        password_hash: row.get(2)?,
        index_doc: row.get(3)?,
        workspace_doc: row.get(4)?,
        created: row.get(5)?,
    })
}

const USER_COLUMNS: &str = "id, name, password_hash, index_doc, workspace_doc, created";

impl Db {
    /// Opens (creating if needed) the database and applies migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA busy_timeout=5000;
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
        db.with(migrate)?;
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

    // ---------- users ----------

    pub fn insert_user(&self, user: &UserRecord) -> Result<()> {
        let user = user.clone();
        self.with(move |connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "INSERT INTO users (id, name, password_hash, index_doc, workspace_doc, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    user.id,
                    user.name,
                    user.password_hash,
                    user.index_doc,
                    user.workspace_doc,
                    user.created
                ],
            )?;
            for doc in [&user.index_doc, &user.workspace_doc] {
                tx.execute(
                    "INSERT INTO acl (doc_id, user_id, role, created) VALUES (?1, ?2, 'owner', ?3)",
                    params![doc, user.id, user.created],
                )?;
            }
            tx.commit()
        })
    }

    pub fn user_by_name(&self, name: &str) -> Result<Option<UserRecord>> {
        let name = name.to_owned();
        self.with(move |connection| {
            connection
                .query_row(
                    &format!("SELECT {USER_COLUMNS} FROM users WHERE name = ?1"),
                    params![name],
                    user_from_row,
                )
                .optional()
        })
    }

    pub fn users(&self) -> Result<Vec<UserRecord>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare(&format!("SELECT {USER_COLUMNS} FROM users ORDER BY name"))?;
            statement.query_map([], user_from_row)?.collect()
        })
    }

    pub fn set_password(&self, user_id: &str, password_hash: &str) -> Result<bool> {
        let (user_id, password_hash) = (user_id.to_owned(), password_hash.to_owned());
        self.with(move |connection| {
            connection
                .execute(
                    "UPDATE users SET password_hash = ?1 WHERE id = ?2",
                    params![password_hash, user_id],
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

    /// The user owning a live (unrevoked) token; records the use.
    pub fn token_user(&self, token_hash: &str) -> Result<Option<UserRecord>> {
        let token_hash = token_hash.to_owned();
        self.with(move |connection| {
            let user = connection
                .query_row(
                    &format!(
                        "SELECT {} FROM tokens t JOIN users u ON u.id = t.user_id
                         WHERE t.token_hash = ?1 AND t.revoked IS NULL",
                        USER_COLUMNS
                            .split(", ")
                            .map(|column| format!("u.{column}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    params![token_hash],
                    user_from_row,
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

    pub fn tokens(&self) -> Result<Vec<TokenRecord>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT t.token_hash, t.user_id, u.name, t.created, t.last_used, t.revoked
                 FROM tokens t JOIN users u ON u.id = t.user_id
                 ORDER BY u.name, t.created",
            )?;
            statement
                .query_map([], |row| {
                    Ok(TokenRecord {
                        token_hash: row.get(0)?,
                        user_id: row.get(1)?,
                        user_name: row.get(2)?,
                        created: row.get(3)?,
                        last_used: row.get(4)?,
                        revoked: row.get(5)?,
                    })
                })?
                .collect()
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
                    if live {
                        tx.query_row(
                            &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"),
                            params![user_id],
                            user_from_row,
                        )
                        .optional()?
                        .map(|user| TicketGrant { user, token_hash })
                    } else {
                        None
                    }
                }
                None => None,
            };
            tx.commit()?;
            Ok(grant)
        })
    }

    // ---------- ACL ----------

    pub fn acl_rows(&self, doc: DocumentId) -> Result<Vec<(String, Role)>> {
        let doc = doc.to_bs58check();
        self.with(move |connection| {
            let mut statement =
                connection.prepare("SELECT user_id, role FROM acl WHERE doc_id = ?1")?;
            statement
                .query_map(params![doc], |row| {
                    let role: String = row.get(1)?;
                    Ok((row.get::<_, String>(0)?, Role::parse(&role)))
                })?
                .collect()
        })
    }

    /// Makes `user_id` the owner of `doc` only if nobody has any row for it.
    /// Returns the rows after the attempt.
    pub fn claim(&self, doc: DocumentId, user_id: &str) -> Result<Vec<(String, Role)>> {
        let doc_text = doc.to_bs58check();
        let user_id = user_id.to_owned();
        self.with(move |connection| {
            connection.execute(
                "INSERT INTO acl (doc_id, user_id, role, created)
                 SELECT ?1, ?2, 'owner', ?3
                 WHERE NOT EXISTS (SELECT 1 FROM acl WHERE doc_id = ?1)",
                params![doc_text, user_id, now_ms()],
            )?;
            Ok(())
        })?;
        self.acl_rows(doc)
    }

    /// Documents `user_id` holds any role on.
    pub fn user_docs(&self, user_id: &str) -> Result<Vec<String>> {
        let user_id = user_id.to_owned();
        self.with(move |connection| {
            let mut statement =
                connection.prepare("SELECT doc_id FROM acl WHERE user_id = ?1 ORDER BY doc_id")?;
            statement
                .query_map(params![user_id], |row| row.get(0))?
                .collect()
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

fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
    let version: usize =
        connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? as usize;
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = connection.transaction()?;
        tx.execute_batch(migration)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str) -> UserRecord {
        UserRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            password_hash: "x".into(),
            index_doc: DocumentId::new().to_bs58check(),
            workspace_doc: DocumentId::new().to_bs58check(),
            created: now_ms(),
        }
    }

    #[test]
    fn migrations_are_idempotent_and_users_get_owner_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        let db = Db::open(&path).unwrap();
        let alice = user("alice");
        db.insert_user(&alice).unwrap();
        assert!(db.insert_user(&user("alice")).is_err(), "names are unique");
        drop(db);
        let db = Db::open(&path).unwrap();
        let index = DocumentId::parse_any(&alice.index_doc).unwrap();
        assert_eq!(db.acl_rows(index).unwrap(), vec![(alice.id, Role::Owner)]);
        let version: i64 = db
            .with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn claim_only_succeeds_for_unowned_documents() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        let (alice, bob) = (user("alice"), user("bob"));
        db.insert_user(&alice).unwrap();
        db.insert_user(&bob).unwrap();
        let doc = DocumentId::new();
        assert_eq!(
            db.claim(doc, &alice.id).unwrap(),
            vec![(alice.id.clone(), Role::Owner)]
        );
        assert_eq!(
            db.claim(doc, &bob.id).unwrap(),
            vec![(alice.id, Role::Owner)],
            "an owned document cannot be claimed by someone else"
        );
    }
}
