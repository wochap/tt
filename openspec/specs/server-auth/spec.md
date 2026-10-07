# server-auth Specification

## Purpose
TBD - created by syncing change server. Admin-created accounts, password login, hashed revocable tokens, websocket tickets.

## Requirements

### Requirement: User accounts are created by an administrator
The server SHALL provide `tt-server user add <name>` which prompts for a password, stores an argon2id hash, and creates the user's index and workspace documents with owner ACL rows. There SHALL be no public registration endpoint.

#### Scenario: Duplicate name
- **WHEN** `user add alice` runs and alice exists
- **THEN** the command fails with exit code 4 and nothing changes

### Requirement: Password login issues a token
`POST /api/login` with `{username, password}` SHALL return `{token, index_doc, user:{id,name}}` on success and 401 otherwise, rate limited to 5 attempts per minute per IP.

#### Scenario: Wrong password
- **WHEN** the password is wrong
- **THEN** the response is 401 with no distinguishing detail, and the attempt is logged

#### Scenario: Rate limit
- **WHEN** a sixth attempt arrives within a minute from one IP
- **THEN** the response is 429

### Requirement: Tokens are stored hashed and revocable
Tokens SHALL be 32 random bytes, stored as SHA-256, accepted as `Authorization: Bearer`, and revocable via `tt-server token revoke`.

#### Scenario: Revoked token
- **WHEN** a revoked token is used on any endpoint or websocket
- **THEN** the request is rejected with 401 and any open websocket for it is closed

### Requirement: Websocket connection tickets
`POST /api/ws-ticket` (authenticated) SHALL return a single-use ticket valid for 60 seconds that may be passed as `?ticket=` on `/sync`; the long-lived token SHALL never be accepted in a URL.

#### Scenario: Ticket reuse
- **WHEN** a ticket is used twice
- **THEN** the second upgrade is rejected with 401
