## ADDED Requirements

### Requirement: HTTP surface
The server SHALL expose `GET /api/health`, `GET /api/me`, `POST /api/login`, `POST /api/logout`, `POST /api/ws-ticket`, `GET /api/export` (the core JSON export schema for the authenticated user), and `GET /sync`.

#### Scenario: Export is per user
- **WHEN** user A calls `/api/export`
- **THEN** the response contains only documents in A's ACL

### Requirement: Static web hosting
When `--web-dir` is set the server SHALL serve its files at `/` with SPA fallback to `index.html` for unknown non-API paths.

#### Scenario: Deep link
- **WHEN** a browser requests `/tasks/42` and no such file exists
- **THEN** `index.html` is returned with status 200

### Requirement: Deployment and TLS
`tt-server serve` SHALL accept `--listen`, `--db`, `--web-dir`, `--tls-cert`, `--tls-key`, and SHALL refuse to start without TLS unless `--insecure-http` is given; a systemd unit SHALL be shipped under `contrib/`.

#### Scenario: Plain HTTP by mistake
- **WHEN** `serve` starts with no TLS flags and no `--insecure-http`
- **THEN** it exits with code 1 and a message explaining both options
