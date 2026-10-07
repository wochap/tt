# sync-core Specification

## Purpose
TBD - created by syncing change core-and-daemon. Forked automerge repository crate, wire protocol, access policy, sharding, and SQLite storage.

## Requirements

### Requirement: Repository crate is forked and generalized
The workspace SHALL contain `crates/automerge_repo`, a fork of fi's crate, with the `FIRP` protocol and bootstrap/root handshake removed and the `StorageAdapter`, `ControlStore`, `NetworkTransport` ports and per-document actor model preserved. Nothing under `/home/gean/Sandboxes/sandbox/fi` SHALL be modified.

#### Scenario: Existing port tests still pass
- **WHEN** `cargo test -p automerge_repo` runs
- **THEN** the storage, document and repository tests carried over from fi pass without the bootstrap tests

### Requirement: Transport speaks the automerge-repo JS wire protocol
The crate SHALL provide a `NetworkTransport` implementation over websocket that encodes and decodes the automerge-repo JS protocol (CBOR messages `join`, `peer`, `sync`, `request`, `unavailable`, `doc-unavailable`, `ephemeral`, `error`) with document ids rendered as bs58check of the 16 id bytes.

#### Scenario: Round trip through the real node sync server
- **WHEN** the integration test starts `@automerge/automerge-repo-sync-server` under node, client A creates a document and flushes, and client B requests that document id
- **THEN** client B receives the document content, and after client B changes it, client A observes the change within 5 seconds

#### Scenario: Node is unavailable
- **WHEN** node or the npm package cannot be started
- **THEN** the test is skipped with a visible message naming the missing component, and never reports a false pass

#### Scenario: Protocol version mismatch
- **WHEN** the peer answers `join` with a `selectedProtocolVersion` other than `"1"`
- **THEN** the transport closes the connection and surfaces a typed error

### Requirement: Access policy port gates every document exchange
The repository SHALL consult an `AccessPolicy` before announcing, requesting, or servicing any document for a peer. The default policy allows everything.

#### Scenario: Policy denies a document
- **WHEN** a peer requests a document the policy marks as not visible to that peer
- **THEN** the repository answers `doc-unavailable` and never sends sync data for it

### Requirement: Document sharding per user and per year
The domain layer SHALL store projects, tags and tasks in one workspace document per user, and time entries in one document per UTC calendar year of the entry start, with an index document listing all document ids.

#### Scenario: Entry crossing new year
- **WHEN** an entry starts on 2026-12-31 23:50 UTC
- **THEN** it is stored in the `entries-2026` document regardless of its end time

#### Scenario: Fresh device joins
- **WHEN** a daemon starts with an index document id in config and an empty store
- **THEN** it requests the index, then every document it lists, and reports ready once the workspace document is loaded

### Requirement: SQLite blob storage adapter
The daemon SHALL persist document snapshots and control state in one SQLite file via a `StorageAdapter` that replaces each key atomically.

#### Scenario: Crash between writes
- **WHEN** the process is killed after one key is written and before the next
- **THEN** on restart every stored document loads to its last fully written snapshot
