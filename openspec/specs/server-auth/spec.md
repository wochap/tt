# server-auth Specification

## Purpose
TBD - created by syncing change server. Admin-created accounts, password login, hashed revocable tokens, websocket tickets.

## Requirements

### Requirement: User accounts are created by an administrator
The server SHALL provide `tt-server user add <name>` which prompts for a password, stores an argon2id hash in the registry account record, and creates the user's index and workspace documents. The server SHALL also provide `user passwd <name>`, `user rename <name|--id id> <new-name>`, `user del <name>` (sets the account's `deleted` time; documents are kept) and `user ls` (showing conflicted and deleted accounts). There SHALL be no public registration endpoint.

#### Scenario: Duplicate name
- **WHEN** `user add alice` runs and a non-deleted alice exists in the registry
- **THEN** the command fails with exit code 4 and nothing changes

#### Scenario: Rename a conflicted account
- **WHEN** `user rename --id <id> bob2` runs for the conflicted second `bob`
- **THEN** the account is named `bob2`, is no longer conflicted, and can log in

#### Scenario: Delete a user
- **WHEN** `user del bob` runs
- **THEN** bob's tokens are revoked, bob's open websockets are closed, and bob's login fails with 401

### Requirement: Password login issues a token
`POST /api/login` with `{username, password}` SHALL return `{token, index_doc, user:{id,name}, server:{id,name}}` on success and 401 otherwise, rate limited to 5 attempts per minute per IP. Deleted accounts SHALL fail like a wrong password. When the password matches an account that is conflicted, the response SHALL be 409 with error code `account_conflict`; this response SHALL only be given after a correct password.

#### Scenario: Wrong password
- **WHEN** the password is wrong
- **THEN** the response is 401 with no distinguishing detail, and the attempt is logged

#### Scenario: Rate limit
- **WHEN** a sixth attempt arrives within a minute from one IP
- **THEN** the response is 429

#### Scenario: Conflicted account
- **WHEN** the user logs in as `bob` with the password of the conflicted second `bob`
- **THEN** the response is 409 with `account_conflict` and no token is issued

### Requirement: Tokens are stored hashed and revocable
Tokens SHALL be 32 random bytes, stored as SHA-256 in the issuing server's local database only, accepted as `Authorization: Bearer`, and revocable via `tt-server token revoke`. When an account becomes deleted, by a local command or a merged registry change, the server SHALL revoke all of that account's tokens.

#### Scenario: Revoked token
- **WHEN** a revoked token is used on any endpoint or websocket
- **THEN** the request is rejected with 401 and any open websocket for it is closed

#### Scenario: Deleted account
- **WHEN** an account is deleted while its daemon holds an open sync websocket
- **THEN** the websocket is closed and the token is rejected with 401 afterwards

### Requirement: Websocket connection tickets
`POST /api/ws-ticket` (authenticated) SHALL return a single-use ticket valid for 60 seconds that may be passed as `?ticket=` on `/sync`; the long-lived token SHALL never be accepted in a URL.

#### Scenario: Ticket reuse
- **WHEN** a ticket is used twice
- **THEN** the second upgrade is rejected with 401
