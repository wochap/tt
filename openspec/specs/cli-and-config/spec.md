# cli-and-config Specification

## Purpose
TBD - created by syncing change core-and-daemon. Daemon lifecycle, socket RPC, config/login, and the CLI JSON output contract.

## Requirements

### Requirement: Daemon lifecycle
`tt daemon` SHALL own the SQLite store under `$XDG_DATA_HOME/tt/`, hold an exclusive lock, and listen on `$XDG_RUNTIME_DIR/tt.sock`. Any other `tt` command SHALL spawn the daemon detached if the socket is absent or dead and wait for readiness up to 5 seconds.

#### Scenario: Second daemon
- **WHEN** a daemon is running and `tt daemon` is started again
- **THEN** the second exits with code 4 and a message naming the lock holder

#### Scenario: Auto spawn
- **WHEN** no daemon is running and the user runs `tt task ls`
- **THEN** the daemon is started, the command succeeds, and the daemon stays alive afterward

### Requirement: Socket RPC
The CLI and daemon SHALL communicate with newline-delimited JSON-RPC 2.0 over the unix socket; every user-facing command maps to one method, and `watch` uses a subscription method streaming notifications.

#### Scenario: Daemon unreachable after spawn
- **WHEN** the daemon fails to become ready
- **THEN** the command exits with code 3 and prints the daemon log path

### Requirement: Config file and login
Configuration SHALL live in `$XDG_CONFIG_HOME/tt/config.toml` with `server.url`, `server.token`, `server.ca_cert`, `user.index_doc`, `week_start`, `snap`, `tz`, `editor`. `tt login <url>` SHALL accept `--ca-cert <pem>`, prompt for username and password, POST `{username,password}` to `<url>/api/login`, store the returned `token`, `index_doc` and `user.id`, store the absolute CA path as `server.ca_cert` when `--ca-cert` is given, and instruct the daemon to connect. The daemon SHALL obtain a websocket ticket via `POST /api/ws-ticket` before each connection and SHALL never place the long-lived token in a URL. On a 401 from any endpoint the daemon SHALL stop syncing and `tt status` SHALL report "login required". `tt logout` SHALL call `/api/logout`, remove the token, and keep local data. With no `server.url` the daemon SHALL never open a network connection.

#### Scenario: Offline usage
- **WHEN** `server.url` is unset
- **THEN** every command works and the daemon makes no outbound connection

#### Scenario: Token stored with restricted permissions
- **WHEN** login succeeds
- **THEN** config.toml is written with mode 0600

#### Scenario: Revoked token
- **WHEN** the server answers 401 to a ticket request
- **THEN** the daemon stops reconnecting and `tt status` reports that login is required

#### Scenario: Login stores the CA path
- **WHEN** `tt login https://tt.example.local --ca-cert ./ca.pem` succeeds
- **THEN** `tt config get server.ca_cert` prints the absolute path of `ca.pem`

### Requirement: JSON output contract and exit codes
Every read command SHALL accept `-j` producing stable JSON with uuids, seqs, UTC timestamps and durations in seconds; exit codes SHALL be 0 ok, 1 usage, 2 not found, 3 daemon unreachable, 4 invalid state or conflict.

#### Scenario: Not found
- **WHEN** `tt task show 9999 -j` is run for a missing task
- **THEN** stdout is empty, stderr has one message, exit code is 2

### Requirement: Additional trusted CA for server connections
Every client connection to the server (login, logout, websocket ticket request, and `wss://` sync) SHALL verify the server certificate against the bundled webpki roots plus, when `server.ca_cert` (or `--ca-cert` during login) is set, every certificate in that PEM file. The extra CA SHALL add to the bundled roots and never replace them. Hostname verification SHALL stay enabled. With `server.ca_cert` unset, only the bundled roots SHALL be trusted.

#### Scenario: Private CA accepted
- **WHEN** the server presents a certificate issued by a private CA and `tt login <url> --ca-cert <that CA>` runs with valid credentials
- **THEN** login succeeds, and the daemon's ticket request and `wss://` sync connect without certificate errors

#### Scenario: Private CA without the option
- **WHEN** the server presents a certificate issued by a private CA and `server.ca_cert` is unset
- **THEN** `tt login` fails with exit code 4 and a message that names the certificate error and suggests `--ca-cert`

#### Scenario: Public certificate still works with a CA configured
- **WHEN** `server.ca_cert` is set and the server presents a certificate from a publicly trusted CA
- **THEN** login and sync succeed

#### Scenario: Hostname mismatch rejected
- **WHEN** the server's certificate is issued by the configured CA for a different hostname
- **THEN** login fails and the daemon does not sync

### Requirement: Invalid CA file fails loudly
A `server.ca_cert` or `--ca-cert` path that cannot be read, or that contains no valid PEM certificate, SHALL be an error naming the file. `tt login --ca-cert` and `tt config set server.ca_cert` SHALL fail with exit code 1 without changing config. When the daemon finds an invalid `server.ca_cert` at start or on reload, it SHALL keep serving local commands, SHALL open no network connection, and `tt status` SHALL report the sync state `ca_cert_invalid` with a message naming the file. The daemon SHALL never fall back to webpki-only trust.

#### Scenario: Missing file at login
- **WHEN** `tt login https://tt.example.local --ca-cert /nope.pem` runs
- **THEN** it exits with code 1, the message names `/nope.pem`, no password is sent, and config.toml is unchanged

#### Scenario: File removed after login
- **WHEN** the configured CA file is deleted and the daemon restarts
- **THEN** local commands keep working, no connection is attempted, and `tt status -j` shows sync state `ca_cert_invalid` naming the path

#### Scenario: Not a certificate
- **WHEN** `tt config set server.ca_cert ./notes.txt` runs and the file has no PEM certificate
- **THEN** the command exits with code 1 and config.toml is unchanged
