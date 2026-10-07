## Context

`core-and-daemon` delivers the Repo fork, the JS-protocol codec with a server-side listener, the `AccessPolicy` port, SQLite storage, and the CLI `login` contract. This change wires them into a hosted process with identity and isolation. Two users, few devices each, one small VPS.

## Goals / Non-Goals

**Goals:**
- One binary, one port, one SQLite file. Trivial to self-host.
- Strict isolation: a user's peer can only learn about and sync documents in their ACL.
- Browser peers use stock `@automerge/automerge-repo` with a websocket adapter plus a token.

**Non-Goals:**
- Public registration, email, password reset flows (admin resets via CLI).
- Sharing documents between users (ACL table supports it; no UI or command yet).
- Horizontal scaling.

## Decisions

### D1. Identity model
`users(id uuid, name unique, password_hash argon2id, created)`, `tokens(token_hash sha256, user_id, created, last_used, revoked)`, `acl(doc_id, user_id, role owner|reader|writer)`. On `user add`, the server creates that user's index + workspace documents in its Repo and inserts owner ACL rows. The index doc id is returned at login so a fresh device can bootstrap (design D3 of core-and-daemon).

### D2. Login and tokens
`POST /api/login {username, password}` → `{token, index_doc, user:{id,name}}`. Tokens are random 32 bytes, stored hashed, long-lived, revocable with `tt-server token revoke`. Rate limit login to 5/min per IP. Argon2id with OWASP parameters. Passwords only ever arrive over TLS; the server refuses to start without TLS unless `--insecure-http` (for a reverse proxy on localhost).

### D3. Websocket sync endpoint
`GET /sync` upgrades to websocket. Auth: `Authorization: Bearer` header (daemon) or `?token=` query (browser cannot set headers on websocket; token is single-use-per-connection and the URL is never logged). After upgrade the server-side `WsJsTransport` listener runs the protocol with `PeerId = user id + connection nonce`. The `AccessPolicy` implementation reads the `acl` table: `visible_docs(peer)` = docs where user has any role; `may_sync(peer, doc)` for writes requires owner|writer. New documents announced by a peer (e.g. a new `entries-2027` doc created offline) are accepted only if the peer's index document lists them after sync, otherwise rejected and dropped; this keeps a peer from pushing arbitrary documents. Simpler rule used for v1: any doc a user's own index lists belongs to that user, ACL rows are created on first sight.

### D4. Server Repo
Same `automerge_repo` crate, `SqliteStorage` on `server.db` (users and ACL in the same file, separate tables). Documents are loaded lazily on first request and evicted after idle (configurable, default 10 min) to bound memory.

### D5. HTTP API
`GET /api/health`, `GET /api/me`, `POST /api/login`, `POST /api/logout`, `GET /api/export` (JSON schema from core, for the authenticated user), static files from `--web-dir` with SPA fallback to `index.html`. All JSON. CORS only needed if web is hosted elsewhere; default same-origin.

### D6. Admin CLI
`tt-server user add <name>` (prompts password), `user passwd`, `user ls`, `token ls|revoke`, `serve --listen --db --web-dir --tls-cert --tls-key`.

## Risks / Trade-offs

- [Token in websocket URL] → short-lived connection ticket: `POST /api/ws-ticket` returns a 60 s single-use ticket used as `?ticket=`; the long-lived token never appears in a URL.
- [A client announces documents that are not theirs] → policy accepts a new doc id only when the user's index lists it; test with a malicious peer.
- [Memory with many open docs] → idle eviction, documents flushed before eviction.
- [Password brute force] → argon2id plus rate limit plus login audit log.

## Open Questions

- None.
