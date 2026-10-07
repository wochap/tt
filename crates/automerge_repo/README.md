# automerge-repo

`automerge-repo` is a Rust repository around Automerge 0.11 that speaks the
[automerge-repo](https://github.com/automerge/automerge-repo) JS wire
protocol. Each loaded document is owned by a bounded Tokio actor; a
coordinator manages persistence, lifecycle, access policy, and peer
replication through application-provided ports.

It is a fork of fi's `automerge_repo` crate; see [`FORK.md`](FORK.md) for the
delta.

## Software stack

- Automerge: conflict-free documents and sync protocol.
- Tokio: actors, channels, storage workers, and lifecycle coordination.
- `ciborium` + `bs58`: the JS protocol's CBOR frames and bs58check document ids.
- `tokio-tungstenite`: websocket client and server transports.
- `rusqlite` (feature `sqlite`, on by default): single-file storage.

## Basic API

```rust
use std::sync::Arc;
use automerge::{ReadDoc, ROOT, transaction::Transactable};
use automerge_repo::{
    Error, Repo, RepoConfig,
    testing::{MemoryStore, MemoryTransport},
};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> automerge_repo::Result<()> {
let documents = Arc::new(MemoryStore::default());
let control = Arc::new(MemoryStore::default());
let (transport, _remote) = MemoryTransport::pair("local", "remote", 128);
let repo = Repo::open(documents, control, transport, RepoConfig::default()).await?;

// Creation is durable and the document is Ready when `create` returns.
let document = repo
    .create_with(|tx| {
        tx.put(ROOT, "title", "Offline first")
            .map_err(|error| Error::Change(error.to_string()))?;
        Ok(())
    })
    .await?;
document
    .change(|tx| {
        tx.put(ROOT, "done", false)
            .map_err(|error| Error::Change(error.to_string()))
    })
    .await?;
let title = document
    .read(|doc| doc.get(ROOT, "title").unwrap().and_then(|(value, _)| value.into_string().ok()))
    .await?;
assert_eq!(title.as_deref(), Some("Offline first"));

// `find` returns a local document, or asks connected peers for it.
let same = repo.find(document.id()).await?;
same.ready().await?;

repo.flush().await?;
repo.shutdown().await?;
# Ok(())
# }
```

Read and change callbacks are synchronous and must not block. Changes are
serialized per document; separate document actors remain independent.

`Repo::find(id)` returns the stored document when there is one. Otherwise it
returns a `Loading` handle and sends a `request` to every connected peer the
policy allows; `DocHandle::ready` resolves once content arrives, or fails
with `LifecycleError::DocumentUnavailable` when every such peer answered
`doc-unavailable` (status `DocumentStatus::Unavailable`). A peer that connects
later is asked again.

## Syncing with automerge-repo JS

`transport::WsJsClient` dials a websocket server such as
`@automerge/automerge-repo-sync-server`, performs the `join`/`peer`
handshake, and reconnects with exponential backoff. `transport::WsJsServer`
accepts JS (or Rust) clients; `listen` serves a `TcpListener`, while
`serve_websocket` and `serve_frames` let an HTTP framework hand over upgraded
connections, optionally with an authenticated identity.

```rust,no_run
use std::sync::Arc;
use automerge_repo::{
    Repo, RepoConfig, SqliteStorage,
    transport::{ConnectionState, WsJsClient, WsJsClientConfig},
};

# async fn example() -> automerge_repo::Result<()> {
let storage = SqliteStorage::open("/var/lib/app/repo.db")?;
let client = WsJsClient::start(
    WsJsClientConfig::new("wss://sync.example.com", "device-1").bearer("token"),
);
let repo = Repo::open(
    Arc::new(storage.clone()),
    Arc::new(storage),
    client.clone(),
    RepoConfig::default(),
)
.await?;
let mut state = client.subscribe_state();
let _ = state
    .wait_for(|state| matches!(state, ConnectionState::Connected { .. }))
    .await;
# let _ = repo;
# Ok(())
# }
```

A protocol version mismatch is not retried: the client ends in
`ConnectionState::Failed(ProtocolError::VersionMismatch { .. })`.

## Access policy

`AccessPolicy` decides which documents a peer may see. `may_sync` gates every
`sync`/`request` in both directions; a denied document is answered with
`doc-unavailable` and no sync data is sent for it. `may_announce` (defaulting
to `may_sync`) decides whether the repository pushes a document to a peer
unprompted, on connect or when the document becomes durable. A server that
only answers explicit requests returns `false` from it.

```rust
use std::sync::Arc;
use automerge_repo::{
    DocumentId, FnPolicy, PeerId, Repo, RepoConfig, transport::WsJsServer,
    testing::MemoryStore,
};

# async fn example(owner_of: fn(DocumentId) -> String) -> automerge_repo::Result<()> {
let server = WsJsServer::new("sync-server");
let policy = FnPolicy(move |peer: &PeerId, document: DocumentId| {
    // With an identity, server peer ids are `<identity>/<senderId>`.
    peer.as_str().split('/').next() == Some(owner_of(document).as_str())
});
let repo = Repo::open_with_policy(
    Arc::new(MemoryStore::default()),
    Arc::new(MemoryStore::default()),
    server,
    RepoConfig::default(),
    Arc::new(policy),
)
.await?;
# let _ = repo;
# Ok(())
# }
```

## Contracts

- `StorageAdapter` stores complete, atomically replaced Automerge snapshots.
  `Repo::open` loads every listed snapshot strictly and fails on the first
  that does not load.
- `ControlStore` is a small key/value store for control state kept apart from
  document bytes.
- `NetworkTransport` performs the handshake, then provides peer ids and
  reliable, ordered, complete frames with bounded backpressure.
- `flush` captures accepted revisions and waits for their durability barriers.
- `shutdown` stops new work, drains accepted commands, flushes, and closes all
  handles.
- `remove_local` is local maintenance, not distributed deletion.
- A lagging event subscriber must rebuild its view with `DocHandle::read`.

Automatic snapshots are debounced and retried after failure. Transaction commit
and peer convergence are not themselves crash-durability guarantees.

## Storage adapters

`SqliteStorage` keeps both ports in one database file: tables
`documents(key, bytes)` (key is the hyphenated document UUID) and
`control(key, bytes)`. It runs in WAL mode with `synchronous=FULL`, and each
`store`/`put` is one `INSERT OR REPLACE` statement, so a key always holds
either its previous or its new complete value, even if the process is killed
mid-write. The file is created with mode `0600` on Unix.

`FilesystemStorage` uses:

```text
<repo>/
├── automerge/<document-uuid>.automerge
└── control/<key>.bin
```

On Unix, directories use mode `0700` and files use `0600`. Replacements use a
temporary sibling, file sync, atomic rename, and directory sync. Control keys
are limited to `[A-Za-z0-9_-]`.

## Build and test

From the repository root:

```sh
cargo build -p automerge-repo
cargo test -p automerge-repo
cargo clippy -p automerge-repo --all-targets --all-features -- -D warnings
```

`tests/js_interop.rs` runs against the real JS sync server and a stock JS
client when node and npm are available; it prints a `SKIPPED` banner
otherwise (set `TT_REQUIRE_NODE=1` to make that a failure).
