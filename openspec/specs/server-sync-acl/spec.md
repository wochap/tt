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
The server's `AccessPolicy` SHALL derive access from the registry: a client user may sync its own index document and every document that index lists, and nothing else. Access SHALL NOT depend on which user first pushed a document. When two users' indexes list the same document id, only the user whose account was created first SHALL have access, and the conflict SHALL be logged. Deleted accounts SHALL have access to nothing. The server SHALL answer `doc-unavailable` for any document a user may not access, and SHALL accept a new document from a client only when that user's index lists it.

#### Scenario: Cross-user request
- **WHEN** user B requests user A's workspace document id
- **THEN** the server answers `doc-unavailable` and sends no sync data, and the attempt is logged

#### Scenario: New yearly entries document
- **WHEN** user A's daemon creates `entries-2027` offline, adds it to A's index, and syncs
- **THEN** the server accepts both documents and A can sync `entries-2027` afterwards

#### Scenario: Unlisted document push
- **WHEN** a peer sends sync data for a document id not listed in its user's index
- **THEN** the data is discarded and the connection is closed with a protocol error

#### Scenario: Two indexes list one document
- **WHEN** user B adds user A's workspace document id to B's own index
- **THEN** B still gets `doc-unavailable` for it, A's access is unchanged, and the conflict is logged

### Requirement: Server document lifecycle
Documents SHALL be loaded lazily, flushed to SQLite after each change, and evicted from memory after a configurable idle period (default 10 minutes).

#### Scenario: Restart loses nothing
- **WHEN** the server is restarted after clients synced changes
- **THEN** reconnecting clients observe all previously synced changes
