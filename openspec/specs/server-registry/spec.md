# server-registry Specification

## Purpose
Gives every tt server one root: a server identity plus a replicated Automerge registry document that holds all accounts, so that servers sharing a root can later converge on the same accounts and data.

## Requirements

### Requirement: Server identity
Every server installation SHALL have an ed25519 keypair and a `server_id` derived from its public key, created on first use of the database directory and stored in a key file readable only by the server's user (mode 0600). The server SHALL also have a human-friendly name, set by `tt-server init --name <name>` and defaulting to the host name. The identity SHALL exist independently of the root.

#### Scenario: Default name
- **WHEN** `tt-server init` runs without `--name` on host `laptop-a`
- **THEN** the server's name is `laptop-a`, and `GET /api/health` reports it with the `server_id`

#### Scenario: Key file permissions
- **WHEN** the key file is created
- **THEN** it is readable and writable only by its owner

### Requirement: Explicit root decision
A server database with no root SHALL be in `NeedsDecision`. `tt-server init` SHALL create the root (the registry document) and move the server to `Ready`. `tt-server serve` SHALL start in `NeedsDecision` in a limited mode: it serves `/api/health` (reporting the state), the web app's not-set-up page, and the admin socket, and it answers every other API route and `/sync` with 503 naming `tt-server init` and `tt-server peer join`. It SHALL log the same hint at start. A database created by an earlier, registry-less version SHALL be refused by every command with exit code 1 and a message saying to move it aside and run `init`; it SHALL NOT be migrated or modified.

#### Scenario: Fresh database
- **WHEN** `tt-server serve` starts on an empty database
- **THEN** it keeps running, `/api/health` reports `NeedsDecision`, `POST /api/login` and `/sync` answer 503 naming `tt-server init`, and the log names `tt-server init` and `tt-server peer join`

#### Scenario: Init while serving
- **WHEN** `tt-server init` runs through the admin socket while `serve` is in `NeedsDecision`
- **THEN** the server moves to `Ready` without a restart and login works

#### Scenario: Init twice
- **WHEN** `tt-server init` runs on a server that already has a root
- **THEN** it exits with code 4 and nothing changes

#### Scenario: Old database
- **WHEN** any `tt-server` command opens a database written by a registry-less version
- **THEN** it exits with code 1, says the format is no longer supported, and the file is unchanged

### Requirement: Accounts live in the registry document
The registry document SHALL hold every account as `id`, `name`, `index_doc`, `workspace_doc`, `password` (argon2id hash with its `changed_at` time), `created`, and `deleted` (absent, or the time of deletion). Login, token issuing and document access SHALL read accounts from the registry. Tokens, websocket tickets, login rate limits and the login audit log SHALL stay in the server's local database and SHALL never be written to the registry.

#### Scenario: Add user writes the registry
- **WHEN** `tt-server user add alice` succeeds
- **THEN** the registry lists alice with a new index and workspace document, and the local token table is unchanged

### Requirement: Deterministic account conflict resolution
Account fields that can be edited concurrently SHALL resolve the same way on every server regardless of merge order: the password with the latest `changed_at` wins, ties broken by the greater hash string; a rename with the latest change time wins, ties broken by the greater name; a set `deleted` time is final and SHALL never be cleared. When two non-deleted accounts share a name, the account with the earlier `created` time (ties: lower id) owns the name and the other account SHALL be flagged as conflicted until renamed.

#### Scenario: Concurrent password changes
- **WHEN** the same account's password is changed on two servers before they sync, at 10:00 and 10:05
- **THEN** after merging, both servers accept only the 10:05 password

#### Scenario: Delete against password change
- **WHEN** one server deletes bob while another changes bob's password, and they sync
- **THEN** bob is deleted on both servers

#### Scenario: Duplicate name created offline
- **WHEN** two servers each create an account named `bob` before syncing
- **THEN** after merging, the earlier-created `bob` logs in normally and the other is listed as conflicted by `tt-server user ls`

### Requirement: Registry is private to the server
The registry document SHALL never be announced, sent, or made available to client peers (daemons, web apps), and client peers SHALL never be able to write it.

#### Scenario: Client requests the registry
- **WHEN** an authenticated client requests the registry document id over `/sync`
- **THEN** the server answers `doc-unavailable` and sends no sync data

### Requirement: Administration while the server runs
Account and token commands (`user add|passwd|rename|del|ls`, `token ls|revoke`) SHALL apply their change through the running server when one is serving the database, and SHALL write the database directly only when no server holds it. A change applied through the running server SHALL take effect immediately, including closing sessions of revoked tokens and deleted users.

#### Scenario: Add user while serving
- **WHEN** `tt-server user add carol` runs while `serve` is running on the same database
- **THEN** carol can log in immediately and no registry change made by the running server is lost

#### Scenario: Server stopped
- **WHEN** `tt-server user passwd alice` runs and no server is running
- **THEN** the password is changed directly in the database and the next `serve` uses it
