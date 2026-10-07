## MODIFIED Requirements

### Requirement: Config file and login
Configuration SHALL live in `$XDG_CONFIG_HOME/tt/config.toml` with `server.url`, `server.token`, `user.index_doc`, `week_start`, `snap`, `tz`, `editor`. `tt login <url>` SHALL prompt for username and password, POST `{username,password}` to `<url>/api/login`, store the returned `token`, `index_doc` and `user.id`, and instruct the daemon to connect. The daemon SHALL obtain a websocket ticket via `POST /api/ws-ticket` before each connection and SHALL never place the long-lived token in a URL. On a 401 from any endpoint the daemon SHALL stop syncing and `tt status` SHALL report "login required". `tt logout` SHALL call `/api/logout`, remove the token, and keep local data. With no `server.url` the daemon SHALL never open a network connection.

#### Scenario: Offline usage
- **WHEN** `server.url` is unset
- **THEN** every command works and the daemon makes no outbound connection

#### Scenario: Token stored with restricted permissions
- **WHEN** login succeeds
- **THEN** config.toml is written with mode 0600

#### Scenario: Revoked token
- **WHEN** the server answers 401 to a ticket request
- **THEN** the daemon stops reconnecting and `tt status` reports that login is required
