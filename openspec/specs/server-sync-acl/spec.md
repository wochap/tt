# server-sync-acl Specification

## Purpose
TBD - created by syncing change server. Automerge-repo sync endpoint with per-user document ACL and server document lifecycle.

## Requirements

### Requirement: Sync endpoint speaks the automerge-repo protocol
`GET /sync` SHALL upgrade to a websocket and run the automerge-repo JS protocol using the server-side transport from `automerge_repo`, with the authenticated user as the peer identity.

#### Scenario: Stock JS client syncs
- **WHEN** a `@automerge/automerge-repo` client with `BrowserWebSocketClientAdapter` connects with a valid ticket and requests the user's workspace document
- **THEN** it receives the document and subsequent changes in both directions

### Requirement: Users only see their own documents
The server's `AccessPolicy` SHALL expose to a peer only documents with an ACL row for that user, SHALL answer `doc-unavailable` for any other id, and SHALL accept a new document from a peer only when the user's index document lists it.

#### Scenario: Cross-user request
- **WHEN** user B requests user A's workspace document id
- **THEN** the server answers `doc-unavailable` and sends no sync data, and the attempt is logged

#### Scenario: New yearly entries document
- **WHEN** user A's daemon creates `entries-2027` offline, adds it to A's index, and syncs
- **THEN** the server accepts both documents and creates an owner ACL row for the new one

#### Scenario: Unlisted document push
- **WHEN** a peer sends sync data for a document id not listed in its user's index
- **THEN** the data is discarded and the connection is closed with a protocol error

### Requirement: Server document lifecycle
Documents SHALL be loaded lazily, flushed to SQLite after each change, and evicted from memory after a configurable idle period (default 10 minutes).

#### Scenario: Restart loses nothing
- **WHEN** the server is restarted after clients synced changes
- **THEN** reconnecting clients observe all previously synced changes
