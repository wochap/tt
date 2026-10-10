//! `server.db`: the root record, tokens, tickets, the login audit log, and
//! the peering state (invite secrets, the join and reset intents, learned
//! peer addresses), next to the `documents`/`control` tables owned by
//! `SqliteStorage` in the same file. Accounts live in the registry document, not here; nothing in
//! these tables is ever replicated.
//!
//! Every record is keyed by text: user ids are UUIDs, documents are bs58check
//! ids, ticket and invite secrets are only ever stored as SHA-256 hex. Tokens
//! are stored by token id (the `tid` of a `tt2` token, which is not a
//! secret: the signature is), in the `token_hash` column of earlier
//! versions. Times are Unix milliseconds.

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
        revoked INTEGER,
        issuer TEXT
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

/// The tables `SqliteStorage` creates, as it creates them; a reset may run
/// before storage ever opened the file.
const STORAGE_TABLES: &str = "
    CREATE TABLE IF NOT EXISTS documents (key TEXT PRIMARY KEY, bytes BLOB NOT NULL);
    CREATE TABLE IF NOT EXISTS control (key TEXT PRIMARY KEY, bytes BLOB NOT NULL);";

/// Peering tables, added to version 2 databases in place (older binaries
/// ignore them).
const PEERING_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS invites (
        secret_hash TEXT PRIMARY KEY,
        created INTEGER NOT NULL,
        expires INTEGER NOT NULL,
        used INTEGER,
        name TEXT
    );
    CREATE TABLE IF NOT EXISTS joining (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        registry_doc TEXT NOT NULL,
        source_id TEXT NOT NULL,
        source_addr TEXT,
        name TEXT NOT NULL,
        started INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS reset_intent (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        new_identity INTEGER NOT NULL,
        started INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS peer_addresses (
        server_id TEXT NOT NULL,
        addr TEXT NOT NULL,
        source TEXT NOT NULL CHECK (source IN ('self', 'hint', 'seed')),
        last_seen INTEGER NOT NULL,
        last_ok INTEGER,
        PRIMARY KEY (server_id, addr)
    );
    CREATE TABLE IF NOT EXISTS peer_seen (
        server_id TEXT PRIMARY KEY,
        last_seen INTEGER NOT NULL,
        address TEXT,
        public_url TEXT
    );";

/// Hint addresses no successful link confirmed for this long are pruned.
pub const HINT_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Hint addresses kept per member.
pub const MAX_HINTS: usize = 16;

/// Where a peer address was learned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressSource {
    /// Advertised by the member itself in its latest hello.
    #[serde(rename = "self")]
    Advertised,
    /// Passed on by another member, or remembered from an invite.
    Hint,
    /// A `--peer` address; never pruned.
    Seed,
}

impl AddressSource {
    fn parse(text: &str) -> Self {
        match text {
            "self" => Self::Advertised,
            "seed" => Self::Seed,
            _ => Self::Hint,
        }
    }
}

/// One known address of a member (`server_id` is empty for a seed whose
/// owner is not known yet). Times are Unix milliseconds of this server's
/// clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerAddress {
    pub server_id: String,
    pub addr: String,
    pub source: AddressSource,
    pub last_seen: i64,
    pub last_ok: Option<i64>,
}

/// When this server last linked with a member, over which address, and
/// the client URL from its latest hello.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenRecord {
    pub last_seen: i64,
    pub address: Option<String>,
    pub public_url: Option<String>,
}

/// Dial order: most recently successful first, then most recently seen,
/// seeds last among the never-successful.
const DIAL_ORDER: &str =
    "ORDER BY last_ok IS NULL, last_ok DESC, source = 'seed', last_seen DESC, addr";

fn address_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PeerAddress> {
    Ok(PeerAddress {
        server_id: row.get(0)?,
        addr: row.get(1)?,
        source: AddressSource::parse(&row.get::<_, String>(2)?),
        last_seen: row.get(3)?,
        last_ok: row.get(4)?,
    })
}

/// Replaces the address table of the first peering version (`addr`,
/// `server_id`, `added`) with the current one, keeping its rows as hints.
fn migrate_peer_addresses(connection: &mut Connection) -> rusqlite::Result<()> {
    let old: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('peer_addresses') WHERE name = 'added')",
        [],
        |row| row.get(0),
    )?;
    if !old {
        return Ok(());
    }
    let tx = connection.transaction()?;
    tx.execute_batch("ALTER TABLE peer_addresses RENAME TO peer_addresses_v1;")?;
    tx.execute_batch(PEERING_SCHEMA)?;
    tx.execute(
        "INSERT OR IGNORE INTO peer_addresses (server_id, addr, source, last_seen)
         SELECT coalesce(server_id, ''), addr,
                CASE WHEN server_id IS NULL THEN 'seed' ELSE 'hint' END, added
         FROM peer_addresses_v1",
        [],
    )?;
    tx.execute_batch("DROP TABLE peer_addresses_v1;")?;
    tx.commit()
}

/// Adds the issuer column to the token table of versions before member-wide
/// tokens, and drops the old opaque tokens with their tickets: they can no
/// longer be verified, so their clients sign in again.
fn migrate_tokens(connection: &mut Connection) -> rusqlite::Result<()> {
    let current: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('tokens') WHERE name = 'issuer')",
        [],
        |row| row.get(0),
    )?;
    if current {
        return Ok(());
    }
    let tx = connection.transaction()?;
    tx.execute_batch(
        "DELETE FROM tickets;
         DELETE FROM tokens;
         ALTER TABLE tokens ADD COLUMN issuer TEXT;",
    )?;
    tx.commit()
}

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

/// A token this server issued or accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    /// The `tid` of the token.
    pub token_id: String,
    pub user_id: String,
    /// The server id of the member that issued it.
    pub issuer: String,
    /// Filled from the registry by [`crate::App::tokens`]; empty here.
    pub user_name: String,
    pub created: i64,
    pub last_used: Option<i64>,
    pub revoked: Option<i64>,
}

impl TokenRecord {
    /// Short public id used by `token ls|revoke`: a prefix of the token id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.token_id[..self.token_id.len().min(12)]
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

/// A join in progress: recorded before anything is fetched, removed in the
/// transaction that records the root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinIntent {
    /// The registry document being fetched.
    pub registry_doc: String,
    /// The server it is fetched from.
    pub source_id: String,
    /// Where to dial it, when this side dials.
    pub source_addr: Option<String>,
    /// This server's name once joined.
    pub name: String,
    pub started: i64,
}

/// Why an invite secret cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteState {
    Valid,
    Expired,
    /// Already used, or never issued here.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteRecord {
    pub state: InviteState,
    pub name: Option<String>,
}

/// Outcome of consuming a websocket ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketGrant {
    pub user_id: String,
    pub token_id: String,
    pub issuer: String,
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
        token_id: row.get(0)?,
        user_id: row.get(1)?,
        issuer: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
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
        db.with(|connection| {
            migrate_tokens(connection)?;
            migrate_peer_addresses(connection)?;
            connection.execute_batch(PEERING_SCHEMA)
        })?;
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

    // ---------- joining ----------

    pub fn joining(&self) -> Result<Option<JoinIntent>> {
        self.with(|connection| {
            connection
                .query_row(
                    "SELECT registry_doc, source_id, source_addr, name, started
                     FROM joining WHERE id = 1",
                    [],
                    |row| {
                        Ok(JoinIntent {
                            registry_doc: row.get(0)?,
                            source_id: row.get(1)?,
                            source_addr: row.get(2)?,
                            name: row.get(3)?,
                            started: row.get(4)?,
                        })
                    },
                )
                .optional()
        })
    }

    /// Records the join intent; `false` (nothing written) when the server
    /// has a root or is already joining.
    pub fn begin_join(&self, intent: &JoinIntent) -> Result<bool> {
        let intent = intent.clone();
        self.with(move |connection| {
            let tx = connection.transaction()?;
            let rooted: bool =
                tx.query_row("SELECT EXISTS (SELECT 1 FROM server)", [], |row| row.get(0))?;
            if rooted {
                return Ok(false);
            }
            let changed = tx.execute(
                "INSERT INTO joining (id, registry_doc, source_id, source_addr, name, started)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5) ON CONFLICT (id) DO NOTHING",
                params![
                    intent.registry_doc,
                    intent.source_id,
                    intent.source_addr,
                    intent.name,
                    intent.started
                ],
            )?;
            tx.commit()?;
            Ok(changed == 1)
        })
    }

    /// Records the root and drops the join intent in one transaction.
    pub fn finish_join(&self, root: &RootRecord) -> Result<()> {
        let root = root.clone();
        self.with(move |connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "INSERT INTO server (id, server_id, name, registry_doc, created)
                 VALUES (1, ?1, ?2, ?3, ?4)",
                params![root.server_id, root.name, root.registry_doc, root.created],
            )?;
            tx.execute("DELETE FROM joining", [])?;
            tx.commit()
        })
    }

    // ---------- invites ----------

    /// Stores an invite secret's hash; `name` is the name this server takes
    /// if it adopts the joiner's root.
    pub fn insert_invite(&self, secret_hash: &str, expires: i64, name: Option<&str>) -> Result<()> {
        let (secret_hash, name) = (secret_hash.to_owned(), name.map(str::to_owned));
        self.with(move |connection| {
            let now = now_ms();
            connection.execute(
                "DELETE FROM invites WHERE expires < ?1 - 86400000",
                params![now],
            )?;
            connection
                .execute(
                    "INSERT INTO invites (secret_hash, created, expires, name) VALUES (?1, ?2, ?3, ?4)",
                    params![secret_hash, now, expires, name],
                )
                .map(|_| ())
        })
    }

    /// An issued invite and whether it could be used now, without using it.
    pub fn invite(&self, secret_hash: &str) -> Result<Option<InviteRecord>> {
        let secret_hash = secret_hash.to_owned();
        self.with(move |connection| {
            let row: Option<(i64, Option<i64>, Option<String>)> = connection
                .query_row(
                    "SELECT expires, used, name FROM invites WHERE secret_hash = ?1",
                    params![secret_hash],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            Ok(row.map(|(expires, used, name)| InviteRecord {
                state: match used {
                    Some(_) => InviteState::Invalid,
                    None if expires <= now_ms() => InviteState::Expired,
                    None => InviteState::Valid,
                },
                name,
            }))
        })
    }

    pub fn invite_state(&self, secret_hash: &str) -> Result<InviteState> {
        Ok(self
            .invite(secret_hash)?
            .map_or(InviteState::Invalid, |invite| invite.state))
    }

    /// Marks an unused, unexpired invite used; returns what it was before.
    pub fn consume_invite(&self, secret_hash: &str) -> Result<InviteState> {
        let hash = secret_hash.to_owned();
        let consumed = self.with(move |connection| {
            connection.execute(
                "UPDATE invites SET used = ?1
                 WHERE secret_hash = ?2 AND used IS NULL AND expires > ?1",
                params![now_ms(), hash],
            )
        })?;
        if consumed == 1 {
            Ok(InviteState::Valid)
        } else {
            match self.invite_state(secret_hash)? {
                InviteState::Valid => Ok(InviteState::Invalid),
                other => Ok(other),
            }
        }
    }

    // ---------- peer addresses ----------

    /// Records a `--peer` seed whose owner is not known yet; a seed already
    /// stored (resolved or not) is left alone. `true` when inserted.
    pub fn add_seed(&self, addr: &str) -> Result<bool> {
        let addr = addr.to_owned();
        self.with(move |connection| {
            let known: bool = connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM peer_addresses WHERE addr = ?1 AND source = 'seed')",
                params![addr],
                |row| row.get(0),
            )?;
            if known {
                return Ok(false);
            }
            connection
                .execute(
                    "INSERT INTO peer_addresses (server_id, addr, source, last_seen)
                     VALUES ('', ?1, 'seed', ?2)",
                    params![addr, now_ms()],
                )
                .map(|changed| changed == 1)
        })
    }

    /// Seeds whose owner is not known yet.
    pub fn unresolved_seeds(&self) -> Result<Vec<String>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT addr FROM peer_addresses WHERE server_id = '' ORDER BY last_seen, addr",
            )?;
            statement.query_map([], |row| row.get(0))?.collect()
        })
    }

    /// Assigns a seed to the member that answered on it.
    pub fn resolve_seed(&self, addr: &str, server_id: &str) -> Result<()> {
        let (addr, server_id) = (addr.to_owned(), server_id.to_owned());
        self.with(move |connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "DELETE FROM peer_addresses WHERE server_id = '' AND addr = ?1",
                params![addr],
            )?;
            tx.execute(
                "INSERT INTO peer_addresses (server_id, addr, source, last_seen)
                 VALUES (?1, ?2, 'seed', ?3)
                 ON CONFLICT (server_id, addr) DO UPDATE SET source = 'seed'",
                params![server_id, addr, now_ms()],
            )?;
            tx.commit()
        })
    }

    /// Stores the addresses a member advertised in a live hello. Its
    /// previous self-advertised addresses that it no longer advertises
    /// become hints (and expire as such); seeds stay seeds.
    pub fn set_advertised(&self, server_id: &str, addrs: &[String], now: i64) -> Result<()> {
        let (server_id, addrs) = (server_id.to_owned(), addrs.to_vec());
        self.with(move |connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "UPDATE peer_addresses SET source = 'hint'
                 WHERE server_id = ?1 AND source = 'self'",
                params![server_id],
            )?;
            for addr in &addrs {
                tx.execute(
                    "INSERT INTO peer_addresses (server_id, addr, source, last_seen)
                     VALUES (?1, ?2, 'self', ?3)
                     ON CONFLICT (server_id, addr) DO UPDATE SET
                         last_seen = excluded.last_seen,
                         source = CASE WHEN source = 'seed' THEN 'seed' ELSE 'self' END",
                    params![server_id, addr, now],
                )?;
            }
            tx.commit()
        })
    }

    /// Adds addresses another member passed on for `server_id`; known ones
    /// keep their times. Keeps the [`MAX_HINTS`] most recent hints.
    pub fn add_hints(&self, server_id: &str, addrs: &[String], now: i64) -> Result<()> {
        let (server_id, addrs) = (server_id.to_owned(), addrs.to_vec());
        self.with(move |connection| {
            let tx = connection.transaction()?;
            for addr in addrs.iter().take(MAX_HINTS) {
                tx.execute(
                    "INSERT INTO peer_addresses (server_id, addr, source, last_seen)
                     VALUES (?1, ?2, 'hint', ?3) ON CONFLICT (server_id, addr) DO NOTHING",
                    params![server_id, addr, now],
                )?;
            }
            tx.execute(
                "DELETE FROM peer_addresses WHERE server_id = ?1 AND source = 'hint'
                 AND addr NOT IN (
                     SELECT addr FROM peer_addresses WHERE server_id = ?1 AND source = 'hint'
                     ORDER BY coalesce(last_ok, last_seen) DESC, addr LIMIT ?2)",
                params![server_id, MAX_HINTS as i64],
            )?;
            tx.commit()
        })
    }

    /// Records a successful link over `addr` to `server_id`.
    pub fn confirm_address(&self, server_id: &str, addr: &str, now: i64) -> Result<()> {
        let (server_id, addr) = (server_id.to_owned(), addr.to_owned());
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO peer_addresses (server_id, addr, source, last_seen, last_ok)
                     VALUES (?1, ?2, 'hint', ?3, ?3)
                     ON CONFLICT (server_id, addr) DO UPDATE SET
                         last_seen = excluded.last_seen, last_ok = excluded.last_ok",
                    params![server_id, addr, now],
                )
                .map(|_| ())
        })
    }

    /// Every known address of a member, in dial order.
    pub fn member_addresses(&self, server_id: &str) -> Result<Vec<PeerAddress>> {
        let server_id = server_id.to_owned();
        self.with(move |connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT server_id, addr, source, last_seen, last_ok FROM peer_addresses
                 WHERE server_id = ?1 {DIAL_ORDER}"
            ))?;
            statement
                .query_map(params![server_id], address_from_row)?
                .collect()
        })
    }

    /// The addresses to dial for a member, in order.
    pub fn dial_order(&self, server_id: &str) -> Result<Vec<String>> {
        Ok(self
            .member_addresses(server_id)?
            .into_iter()
            .map(|address| address.addr)
            .collect())
    }

    /// Removes hints that no successful link confirmed within
    /// [`HINT_TTL_MS`] before `now`; seeds and self-advertised addresses
    /// stay. Returns how many were removed.
    pub fn prune_hints(&self, now: i64) -> Result<usize> {
        self.with(move |connection| {
            connection.execute(
                "DELETE FROM peer_addresses
                 WHERE source = 'hint' AND coalesce(last_ok, last_seen) < ?1",
                params![now - HINT_TTL_MS],
            )
        })
    }

    /// Records a link with a member: when, over which address (kept when
    /// `None`), and its client URL from the hello.
    pub fn record_link(
        &self,
        server_id: &str,
        address: Option<&str>,
        public_url: Option<&str>,
        now: i64,
    ) -> Result<()> {
        let (server_id, address, public_url) = (
            server_id.to_owned(),
            address.map(str::to_owned),
            public_url.map(str::to_owned),
        );
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO peer_seen (server_id, last_seen, address, public_url)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (server_id) DO UPDATE SET
                         last_seen = excluded.last_seen,
                         address = coalesce(excluded.address, address),
                         public_url = excluded.public_url",
                    params![server_id, now, address, public_url],
                )
                .map(|_| ())
        })
    }

    /// Moves a member's last-seen time (a link that just ended).
    pub fn touch_link(&self, server_id: &str, now: i64) -> Result<()> {
        let server_id = server_id.to_owned();
        self.with(move |connection| {
            connection
                .execute(
                    "UPDATE peer_seen SET last_seen = ?2 WHERE server_id = ?1",
                    params![server_id, now],
                )
                .map(|_| ())
        })
    }

    pub fn peer_seen(&self) -> Result<std::collections::HashMap<String, SeenRecord>> {
        self.with(|connection| {
            let mut statement = connection
                .prepare("SELECT server_id, last_seen, address, public_url FROM peer_seen")?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        SeenRecord {
                            last_seen: row.get(1)?,
                            address: row.get(2)?,
                            public_url: row.get(3)?,
                        },
                    ))
                })?
                .collect()
        })
    }

    // ---------- reset ----------

    /// Records the reset intent (write-ahead).
    pub fn begin_reset(&self, new_identity: bool) -> Result<()> {
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO reset_intent (id, new_identity, started) VALUES (1, ?1, ?2)
                     ON CONFLICT (id) DO UPDATE SET new_identity = max(new_identity, excluded.new_identity)",
                    params![new_identity, now_ms()],
                )
                .map(|_| ())
        })
    }

    /// The outstanding reset intent: `Some(new_identity)`.
    pub fn reset_intent(&self) -> Result<Option<bool>> {
        self.with(|connection| {
            connection
                .query_row(
                    "SELECT new_identity FROM reset_intent WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
        })
    }

    /// Deletes every document snapshot (the bulk of a reset).
    pub fn reset_documents(&self) -> Result<()> {
        self.with(|connection| {
            connection.execute_batch(STORAGE_TABLES)?;
            connection.execute("DELETE FROM documents", []).map(|_| ())
        })
    }

    /// Deletes the root, control rows, tokens, tickets, invite secrets, peer
    /// state and the join intent, and the reset intent with them.
    pub fn finish_reset(&self) -> Result<()> {
        self.with(|connection| {
            connection.execute_batch(STORAGE_TABLES)?;
            let tx = connection.transaction()?;
            tx.execute_batch(
                "DELETE FROM documents;
                 DELETE FROM control;
                 DELETE FROM server;
                 DELETE FROM tickets;
                 DELETE FROM tokens;
                 DELETE FROM invites;
                 DELETE FROM joining;
                 DELETE FROM peer_addresses;
                 DELETE FROM peer_seen;
                 DELETE FROM reset_intent;",
            )?;
            tx.commit()
        })
    }

    // ---------- tokens ----------

    /// Records a token this server issued.
    pub fn insert_token(&self, token_id: &str, user_id: &str, issuer: &str) -> Result<()> {
        let (token_id, user_id, issuer) =
            (token_id.to_owned(), user_id.to_owned(), issuer.to_owned());
        self.with(move |connection| {
            connection
                .execute(
                    "INSERT INTO tokens (token_hash, user_id, created, issuer)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![token_id, user_id, now_ms(), issuer],
                )
                .map(|_| ())
        })
    }

    /// Records the use of a verified token, adding it when another member
    /// issued it (`created` is its issue time); `false` when this server
    /// has revoked it.
    pub fn touch_token(
        &self,
        token_id: &str,
        user_id: &str,
        issuer: &str,
        created: i64,
    ) -> Result<bool> {
        let (token_id, user_id, issuer) =
            (token_id.to_owned(), user_id.to_owned(), issuer.to_owned());
        self.with(move |connection| {
            connection.execute(
                "INSERT OR IGNORE INTO tokens (token_hash, user_id, created, issuer)
                 VALUES (?1, ?2, ?3, ?4)",
                params![token_id, user_id, created, issuer],
            )?;
            connection
                .query_row(
                    "UPDATE tokens SET last_used = ?1 WHERE token_hash = ?2
                     RETURNING revoked IS NULL",
                    params![now_ms(), token_id],
                    |row| row.get(0),
                )
                .optional()
                .map(|live| live.unwrap_or(false))
        })
    }

    /// The user id of a live (unrevoked) token.
    pub fn token_user(&self, token_id: &str) -> Result<Option<String>> {
        let token_id = token_id.to_owned();
        self.with(move |connection| {
            connection
                .query_row(
                    "SELECT user_id FROM tokens WHERE token_hash = ?1 AND revoked IS NULL",
                    params![token_id],
                    |row| row.get(0),
                )
                .optional()
        })
    }

    /// Revokes one token; `false` when it was unknown or already revoked.
    pub fn revoke_token(&self, token_id: &str) -> Result<bool> {
        let token_id = token_id.to_owned();
        self.with(move |connection| {
            connection
                .execute(
                    "UPDATE tokens SET revoked = ?1 WHERE token_hash = ?2 AND revoked IS NULL",
                    params![now_ms(), token_id],
                )
                .map(|changed| changed == 1)
        })
    }

    /// Marks known tokens revoked (revocations learned from the registry).
    pub fn mark_revoked(&self, token_ids: Vec<String>) -> Result<()> {
        self.with(move |connection| {
            let tx = connection.transaction()?;
            {
                let mut statement = tx.prepare(
                    "UPDATE tokens SET revoked = ?1 WHERE token_hash = ?2 AND revoked IS NULL",
                )?;
                let now = now_ms();
                for id in token_ids {
                    statement.execute(params![now, id])?;
                }
            }
            tx.commit()
        })
    }

    /// Revokes every live token of a user; returns the revoked token ids.
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
                "SELECT token_hash, user_id, created, last_used, revoked, issuer
                 FROM tokens ORDER BY created",
            )?;
            statement.query_map([], token_from_row)?.collect()
        })
    }

    /// Of `token_ids`, the ones that are no longer live (revoked or unknown).
    pub fn dead_tokens(&self, token_ids: Vec<String>) -> Result<Vec<String>> {
        self.with(move |connection| {
            let mut statement = connection
                .prepare("SELECT 1 FROM tokens WHERE token_hash = ?1 AND revoked IS NULL")?;
            let mut dead = Vec::new();
            for id in token_ids {
                if !statement.exists(params![id])? {
                    dead.push(id);
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
        token_id: &str,
        expires: i64,
    ) -> Result<()> {
        let (ticket_hash, user_id, token_id) = (
            ticket_hash.to_owned(),
            user_id.to_owned(),
            token_id.to_owned(),
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
                    params![ticket_hash, user_id, token_id, expires],
                )
                .map(|_| ())
        })
    }

    /// Marks a ticket used and returns its grant if it was unused, unexpired,
    /// and its token is still live here. A second call for the same ticket
    /// fails. The caller still checks the token against the registry.
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
                Some((user_id, token_id)) => tx
                    .query_row(
                        "SELECT coalesce(issuer, '') FROM tokens
                         WHERE token_hash = ?1 AND revoked IS NULL",
                        params![token_id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .map(|issuer| TicketGrant {
                        user_id,
                        token_id,
                        issuer,
                    }),
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
        assert_eq!(
            tables,
            [
                "invites",
                "joining",
                "login_audit",
                "peer_addresses",
                "peer_seen",
                "reset_intent",
                "server",
                "tickets",
                "tokens"
            ]
        );
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
        db.insert_token("h1", "alice", "srv").unwrap();
        db.insert_token("h2", "alice", "srv").unwrap();
        db.insert_token("h3", "bob", "srv").unwrap();
        assert!(db.revoke_token("h1").unwrap());
        assert_eq!(db.revoke_user_tokens("alice").unwrap(), ["h2"]);
        assert_eq!(db.token_user("h3").unwrap().as_deref(), Some("bob"));
        assert_eq!(db.token_user("h2").unwrap(), None);
    }

    #[test]
    fn tokens_of_other_issuers_are_recorded_on_use() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        assert!(db.touch_token("t1", "alice", "server-a", 5).unwrap());
        assert!(db.touch_token("t1", "alice", "server-a", 5).unwrap());
        let tokens = db.tokens().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            (tokens[0].issuer.as_str(), tokens[0].created),
            ("server-a", 5)
        );
        assert!(tokens[0].last_used.is_some());
        db.mark_revoked(vec!["t1".into(), "unknown".into()])
            .unwrap();
        assert!(!db.touch_token("t1", "alice", "server-a", 5).unwrap());
    }

    #[test]
    fn opaque_tokens_are_dropped_when_the_issuer_column_is_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        let db = Db::open(&path).unwrap();
        db.insert_token("t1", "alice", "srv").unwrap();
        db.with(|c| {
            c.execute_batch(
                "DELETE FROM tokens;
                 ALTER TABLE tokens DROP COLUMN issuer;
                 INSERT INTO tokens (token_hash, user_id, created) VALUES ('old', 'alice', 1);",
            )
        })
        .unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db.tokens().unwrap().is_empty());
        db.insert_token("t2", "alice", "srv").unwrap();
        assert_eq!(db.tokens().unwrap()[0].issuer, "srv");
    }

    #[test]
    fn invites_are_single_use_and_expire() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        db.insert_invite("live", now_ms() + 60_000, Some("cloud"))
            .unwrap();
        db.insert_invite("old", now_ms() - 1, None).unwrap();
        assert_eq!(
            db.invite("live").unwrap().unwrap().name.as_deref(),
            Some("cloud")
        );
        assert_eq!(db.invite_state("live").unwrap(), InviteState::Valid);
        assert_eq!(db.consume_invite("live").unwrap(), InviteState::Valid);
        assert_eq!(db.consume_invite("live").unwrap(), InviteState::Invalid);
        assert_eq!(db.consume_invite("old").unwrap(), InviteState::Expired);
        assert_eq!(db.consume_invite("never").unwrap(), InviteState::Invalid);
    }

    #[test]
    fn join_intent_is_refused_with_a_root_and_finishes_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        let intent = JoinIntent {
            registry_doc: "reg".into(),
            source_id: "src".into(),
            source_addr: Some("a:1".into()),
            name: "b".into(),
            started: 1,
        };
        assert!(db.begin_join(&intent).unwrap());
        assert!(!db.begin_join(&intent).unwrap(), "one join at a time");
        assert_eq!(db.joining().unwrap(), Some(intent.clone()));
        let root = RootRecord {
            server_id: "me".into(),
            name: "b".into(),
            registry_doc: "reg".into(),
            created: 2,
        };
        db.finish_join(&root).unwrap();
        assert_eq!(db.joining().unwrap(), None);
        assert_eq!(db.root().unwrap(), Some(root));
        assert!(
            !db.begin_join(&intent).unwrap(),
            "a rooted server never joins"
        );
    }

    #[test]
    fn addresses_dial_by_success_then_sight_with_seeds_last() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        let now = now_ms();
        db.add_seed("seed.example:8772").unwrap();
        db.resolve_seed("seed.example:8772", "b").unwrap();
        db.set_advertised("b", &["new.lan:8772".into()], now - 10)
            .unwrap();
        db.add_hints("b", &["old.hint:8772".into()], now - 1_000)
            .unwrap();
        db.add_hints("b", &["work.lan:8772".into()], now - 20)
            .unwrap();
        db.confirm_address("b", "work.lan:8772", now - 500).unwrap();
        db.set_advertised("b", &["tail.ts.net:8772".into()], now)
            .unwrap();
        db.confirm_address("b", "tail.ts.net:8772", now).unwrap();
        db.add_hints("c", &["c.lan:8772".into()], now).unwrap();
        assert_eq!(
            db.dial_order("b").unwrap(),
            [
                "tail.ts.net:8772",
                "work.lan:8772",
                "new.lan:8772",
                "old.hint:8772",
                "seed.example:8772"
            ]
        );
        // The address no longer advertised became a hint.
        let sources: Vec<(String, AddressSource)> = db
            .member_addresses("b")
            .unwrap()
            .into_iter()
            .map(|a| (a.addr, a.source))
            .collect();
        assert!(sources.contains(&("new.lan:8772".into(), AddressSource::Hint)));
        assert!(sources.contains(&("tail.ts.net:8772".into(), AddressSource::Advertised)));
        assert!(sources.contains(&("seed.example:8772".into(), AddressSource::Seed)));
    }

    #[test]
    fn unconfirmed_hints_expire_after_30_days_and_seeds_stay() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        let day = 24 * 60 * 60 * 1000;
        let start = now_ms() - 40 * day;
        db.add_seed("seed:1").unwrap();
        db.resolve_seed("seed:1", "b").unwrap();
        db.add_seed("never-answered:1").unwrap();
        db.add_hints("b", &["stale:1".into(), "used:1".into()], start)
            .unwrap();
        db.confirm_address("b", "used:1", start + 20 * day).unwrap();
        db.add_hints("b", &["fresh:1".into()], start + 35 * day)
            .unwrap();
        // Hinted again later: the first sighting counts.
        db.add_hints("b", &["stale:1".into()], start + 39 * day)
            .unwrap();
        let removed = db.prune_hints(start + 40 * day).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(db.dial_order("b").unwrap(), ["used:1", "fresh:1", "seed:1"]);
        assert_eq!(db.unresolved_seeds().unwrap(), ["never-answered:1"]);
        // Seeds survive any age.
        db.prune_hints(start + 4000 * day).unwrap();
        assert_eq!(db.dial_order("b").unwrap(), ["seed:1"]);
    }

    #[test]
    fn hints_are_capped_per_member() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("server.db")).unwrap();
        let now = now_ms();
        let many: Vec<String> = (0..40).map(|i| format!("10.0.0.{i}:8772")).collect();
        db.add_hints("b", &many, now).unwrap();
        assert_eq!(db.dial_order("b").unwrap().len(), MAX_HINTS);
        db.add_hints("b", &["later:1".into()], now + 1).unwrap();
        let order = db.dial_order("b").unwrap();
        assert_eq!(order.len(), MAX_HINTS);
        assert_eq!(order[0], "later:1");
    }

    #[test]
    fn seeds_load_once_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        for _ in 0..3 {
            let db = Db::open(&path).unwrap();
            for seed in ["a.example:8772", "b.example:8772"] {
                db.add_seed(seed).unwrap();
            }
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(
            db.unresolved_seeds().unwrap(),
            ["a.example:8772", "b.example:8772"]
        );
        // A resolved seed is not added again as an unresolved one.
        db.resolve_seed("a.example:8772", "srv-a").unwrap();
        assert!(!db.add_seed("a.example:8772").unwrap());
        assert_eq!(db.unresolved_seeds().unwrap(), ["b.example:8772"]);
        assert_eq!(db.dial_order("srv-a").unwrap(), ["a.example:8772"]);
    }

    #[test]
    fn first_version_address_rows_become_hints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        Db::open(&path).unwrap();
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "DROP TABLE peer_addresses;
                 CREATE TABLE peer_addresses (addr TEXT PRIMARY KEY, server_id TEXT, added INTEGER NOT NULL);
                 INSERT INTO peer_addresses VALUES ('a:1', 'srv-a', 5);",
            )
            .unwrap();
        let db = Db::open(&path).unwrap();
        let addresses = db.member_addresses("srv-a").unwrap();
        assert_eq!(addresses.len(), 1);
        assert_eq!(
            (addresses[0].source, addresses[0].last_seen),
            (AddressSource::Hint, 5)
        );
    }
}
