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
Configuration SHALL live in `$XDG_CONFIG_HOME/tt/config.toml` with `server.url`, `server.token`, `user.index_doc`, `week_start`, `snap`, `tz`, `editor`. `tt login <url>` SHALL prompt for username and password, POST `{username,password}` to `<url>/api/login`, store the returned `token` and `index_doc`, and instruct the daemon to connect. With no `server.url` the daemon SHALL never open a network connection.

#### Scenario: Offline usage
- **WHEN** `server.url` is unset
- **THEN** every command works and the daemon makes no outbound connection

#### Scenario: Token stored with restricted permissions
- **WHEN** login succeeds
- **THEN** config.toml is written with mode 0600

### Requirement: JSON output contract and exit codes
Every read command SHALL accept `-j` producing stable JSON with uuids, seqs, UTC timestamps and durations in seconds; exit codes SHALL be 0 ok, 1 usage, 2 not found, 3 daemon unreachable, 4 invalid state or conflict.

#### Scenario: Not found
- **WHEN** `tt task show 9999 -j` is run for a missing task
- **THEN** stdout is empty, stderr has one message, exit code is 2
