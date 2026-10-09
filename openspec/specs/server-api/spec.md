# server-api Specification

## Purpose
TBD - created by syncing change server. HTTP surface, static web hosting, deployment and TLS for the tt server.

## Requirements

### Requirement: HTTP surface
The server SHALL expose `GET /api/health`, `GET /api/me`, `POST /api/login`, `POST /api/logout`, `POST /api/ws-ticket`, `GET /api/export` (the core JSON export schema for the authenticated user), and `GET /sync`. `GET /api/health` (unauthenticated) and `GET /api/me` SHALL include `server: {id, name}`.

#### Scenario: Export is per user
- **WHEN** user A calls `/api/export`
- **THEN** the response contains only A's index document and the documents it lists

#### Scenario: Server identity
- **WHEN** a client calls `GET /api/health` on server `laptop-a`
- **THEN** the response includes `server.name` `laptop-a` and its `server.id`

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
