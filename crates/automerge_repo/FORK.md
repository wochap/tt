# Fork notes: tt `automerge-repo` vs fi `automerge_repo`

This crate started as a copy of `fi/crates/automerge_repo`. The actor model
(one bounded actor per document, one coordinator), persistence pipeline
(debounced snapshots, retry with backoff, flush linearization, ordered
removal), shutdown aggregation, peer sync progress, and the bounded per-peer
writer are unchanged. What changed is everything about *who talks to whom and
how*: fi's private FIRP protocol and root-document bootstrap are replaced by
the automerge-repo JS wire protocol, so this repository interoperates with
`@automerge/automerge-repo` clients and `automerge-repo-sync-server`.

## Removed

- **FIRP protocol** (`protocol::Codec`, `protocol::Message`, `BootstrapMode`,
  magic/version header, `Hello`/`BootstrapState`/`Inventory`/`Announce`/`Sync`
  frames) and its errors (`ProtocolError::{InvalidLength, BadMagic,
  UnsupportedVersion, UnknownKind, ReservedFlags, UnknownBootstrapMode,
  InvalidInventoryCount, HelloRequired, DuplicateHello}`).
- **Bootstrap state machine**: module `bootstrap` (`BootstrapRecord`,
  `BootstrapStatus`, `BootstrapOffer`), `Repo::{initialize_new, join_existing,
  bootstrap_status, subscribe_bootstrap, bootstrap_offers, subscribe_offers}`,
  `BootstrapError`, `Error::Bootstrap`, the root document and every gate on it
  (writes before a decision, root-only sync, root removal refusal,
  `RootMismatch` isolation, `Joining` root that never becomes `Ready`).
- **Recovery and quarantine**: module `recovery` (`Classification`,
  `QuarantineEntry`, `QuarantineReason`, `RecoveryRecord`, `RecoveryOutcome`,
  `RecoveryReason`, `BootstrapCondition`), `Repo::{recovery,
  subscribe_recovery}`, `StorageAdapter::{quarantine, quarantined,
  load_quarantined, discard_quarantined}`, `ControlStore::{recovery_attempts,
  record_recovery_attempt}`, and the matching `MemoryStore` test hooks.
  `Repo::open` is now strict: every listed snapshot must load or `open` fails
  with `Error::Automerge { document, .. }`, leaving the bytes untouched.
- `ActorHandle::mark_ready` (only the bootstrap used it).

## Added

- `protocol`: `WireMessage` (CBOR codec for `join`, `peer`, `error`, `sync`,
  `request`, `doc-unavailable` (also decodes legacy `unavailable`),
  `ephemeral`, and `Other` for well-formed types this crate ignores such as
  `remote-heads-changed`), `PeerMetadata`, `PROTOCOL_VERSION = "1"`,
  `MAX_FRAME_LEN` (64 MiB), and `retarget` (rewrite `targetId` in an encoded
  frame). Decoding tolerates `cbor-x` output: 16-bit map headers, `undefined`,
  and tag-64 typed arrays.
- `ids`: `DocumentId::{to_bs58check, from_bs58check, parse_any, to_url}` and
  `InvalidDocumentId`. Internally ids are still 16-byte UUIDs (storage keys
  and `Display` are hyphenated UUIDs); on the wire they are bs58check of the
  16 bytes, as JS `stringifyAutomergeUrl` produces.
- `transport::ws_js`: `WsJsClient` / `WsJsClientConfig` / `ConnectionState`
  and `WsJsServer` (see below).
- `policy`: `AccessPolicy`, `AllowAll`, `FnPolicy`.
- `sqlite` (feature `sqlite`, default on): `SqliteStorage`, implementing both
  persistence ports in one WAL-mode, `synchronous=FULL` database.
- `Repo::{open_with_policy, find, local_peer, subscribe_peers,
  connected_peers}`.
- `DocumentStatus::Unavailable`, `LifecycleError::DocumentUnavailable`.
- `ProtocolError::{Cbor, NotAMap, MissingField, InvalidField, Unencodable,
  UnexpectedHandshake, VersionMismatch, Remote}`, `NetworkError::NotConnected`.
- `NetworkTransport::local_peer()`: the id the repository writes as
  `senderId`.

## Changed APIs

- `Repo::open(storage, control, transport, config)` keeps its signature and
  uses `AllowAll`; `open_with_policy` adds an `Arc<dyn AccessPolicy>`.
- `Repo::create` / `create_with` work immediately on a fresh repository (no
  bootstrap decision). Creation is still hidden until its first snapshot is
  stored and flushed; the handle is `Ready` when the call returns.
- `ControlStore` is a small key/value store: `get(key)`, `put(key, value)`,
  `flush`, `close`. `FilesystemStorage` stores each key at
  `control/<key>.bin` (keys limited to `[A-Za-z0-9_-]`); `SqliteStorage` uses
  a `control` table. The repository itself only flushes and closes it today;
  the KV is for applications (for example a storage id).
- `NetworkTransport` implementations own the handshake. `PeerConnected` is
  emitted only after `join`/`peer` completes and `Message` frames carry
  repository messages only. `MemoryTransport` and `MemoryNetwork` have no
  handshake at all: `connect()` emits `PeerConnected` directly and frames are
  raw JS-protocol messages.
- `DocumentStatus` gained `Unavailable`; `DocHandle::ready` fails with
  `LifecycleError::DocumentUnavailable` in that state.
- `RelationshipSyncState::Synced` now means "the peer acknowledged exactly
  our heads" (`have_responded` and `their_heads == heads`). fi also required
  `!in_flight` and `last_sent_heads == heads`; a JS peer sends no reply to a
  final acknowledgement, so `in_flight` stayed set and progress reported
  `Syncing` forever after any remote change.

## Wire protocol mapping

| Repository event | Frame |
| --- | --- |
| Peer connected | For each `Ready` document with `may_announce(peer, doc)`: attach the peer and send `sync`. For each document still being looked for with `may_sync`: send `request`. |
| Document becomes durable (create, or first content received) | Attach every connected peer with `may_announce` and send `sync`. |
| Local or remote change | `sync` to every attached peer, as Automerge's sync state requires. |
| `find(id)` for a document not stored locally | `Loading` handle; `request` to every connected peer with `may_sync`. |

**`sync` vs `request`.** The actor picks the type per outgoing message: when
the local document has no history (empty heads) the message is a `request`,
otherwise it is a `sync`. That matches JS, where a repo that lacks the
document asks for it with `request` and every later exchange is `sync`.

Inbound handling, in order:

1. `may_sync(peer, doc)` is false: reply `doc-unavailable`; nothing is
   applied and no actor is created.
2. `request` for a document with no local history: reply
   `doc-unavailable`. A document whose content has arrived but whose first
   snapshot is still being made durable (`Loading` with history) is served,
   so a relay never reports a document it holds as unavailable.
3. `sync` for an unknown document: create a `Loading` actor and apply it
   (this is how announcements arrive). Once the first content is durable the
   document becomes `Ready` and is announced to the other peers, which is how
   multi-peer relay works.
4. Otherwise the sync message is applied by the document's actor and any
   reply goes back as `sync`/`request` per the rule above.

**`doc-unavailable`.** Ignored for a `Ready` document. Otherwise the sender is
detached from the document and recorded as lacking it; once every connected
peer the policy allows has said so, the handle moves to `Unavailable`. A peer
that connects later is asked again (`Unavailable` -> `Loading`), and content
from any peer moves it to `Ready`. `find` with no eligible peer connected
goes straight to `Unavailable`.

**Requests are not forwarded.** A JS repo that receives a `request` for a
document it does not have forwards the request to its other peers and
answers only when they do. This repository answers `doc-unavailable`
immediately. A sync server built on it therefore serves only what it already
holds; relaying still happens for documents that reach the server through
`sync`.

Other inbound frames: `error` is published on `subscribe_errors` as
`ProtocolError::Remote`. `join`/`peer` after the handshake, undecodable CBOR,
or an undecodable Automerge sync payload are protocol failures: the error is
published, the peer's sync state is dropped, and the transport closes the
peer. `ephemeral`, `remote-heads-changed`, `remote-subscription-change` and
unknown types are ignored. If a peer's bounded writer queue is full, the peer
is dropped and closed (sync states would otherwise be out of step); a fresh
session resyncs.

## Transports

**Handshake.** The client sends `join` with `supportedProtocolVersions:
["1"]` and `peerMetadata: { isEphemeral: false }`. The server accepts a join
that lists `"1"` (or lists nothing) and replies `peer` with
`selectedProtocolVersion: "1"`. Otherwise it sends `error` ("unsupported
protocol version"), closes the socket, and never reports the peer to the
repository.

**`WsJsClient`** reconnects with exponential backoff between `min_backoff`
and `max_backoff`, resetting after a connection that stayed up longer than
`max_backoff`. States: `Connecting`, `Connected { remote }`,
`Disconnected { error, retry_in }`, `Failed(..)`, `Closed`. A `peer` that
selects another version ends in
`Failed(ProtocolError::VersionMismatch { selected })`, and an `error` reply
during the handshake ends in `Failed(ProtocolError::Remote(..))`; neither is
retried. Each reconnection is a new `PeerConnected`, so sync states start
fresh. `close_peer` drops the socket and lets the loop reconnect.

**`WsJsServer`** keeps one session per repository-facing peer id; a new
session with the same id replaces the old one. With `identity: None` the peer
id is the client's `senderId`. With `identity: Some(identity)` (an
authenticated server) the repository sees `<identity>/<senderId>`, so two
users cannot collide or impersonate each other by choosing a `senderId`. The
repository writes that scoped id as `targetId`. **Retargeting:**
`WsJsServer::send` rewrites `targetId` back to the client's wire `senderId`
(`protocol::retarget`) whenever the two differ, so JS clients see their own
id.

## AccessPolicy

`may_sync(peer, doc)` is the hard gate. It is consulted:

- on every inbound `sync` and `request`, before decoding the payload (denied:
  `doc-unavailable`);
- on every outbound sync message produced by a document actor, just before
  it is framed (denied: dropped), so a policy that changes at runtime takes
  effect on the next message;
- when `find` chooses which peers to ask, when a newly connected peer is
  asked for documents still being looked for, and when deciding whether
  every eligible peer has answered `doc-unavailable`.

`may_announce(peer, doc)` (default: `may_sync`) only decides whether the
repository pushes a document unprompted, on connect and when a document
becomes durable. Returning `false` while `may_sync` is `true` gives
request-only serving: the peer must `find` the document by id.

Policies are called on the coordinator task and must be fast and
non-blocking.

## Tests

Kept and adapted from fi: storage ports, document change/rollback/events,
persistence (debounce, retries, flush aggregation, blocked stores, removal),
shutdown, filesystem layout and permissions, offline convergence, multi-peer
relay, disconnect/reconnect, peer sync progress, and the bounded peer writer.
Dropped: tests that only covered bootstrap, root, join, recovery,
quarantine, `Hello`, or inventory. New: `tests/policy.rs` (deny paths seen
from a raw JS-protocol peer), `tests/sqlite.rs` (SIGKILL mid-write and Repo
restart), `tests/ws_transport.rs` (version mismatch, reconnect, server error
and identity retargeting), and `tests/js_interop.rs` (real JS sync server and
client).
