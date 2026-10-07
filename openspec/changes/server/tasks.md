## 1. Crate and storage

- [ ] 1.1 Create `crates/tt-server` (binary `tt-server`) with axum, tokio-tungstenite, argon2, rusqlite; share `SqliteStorage` from `automerge_repo`
- [ ] 1.2 Schema migrations for `users`, `tokens`, `tickets`, `acl`, `login_audit` tables in `server.db`
- [ ] 1.3 Admin CLI: `user add|passwd|ls`, `token ls|revoke`; `user add` creates index + workspace docs and owner ACL rows

## 2. Auth

- [ ] 2.1 `POST /api/login` with argon2id verify, token issue (32 random bytes, sha256 stored), rate limit 5/min/IP, audit log
- [ ] 2.2 Bearer extractor middleware; `GET /api/me`; `POST /api/logout` revokes
- [ ] 2.3 `POST /api/ws-ticket` single-use 60 s tickets; tests for reuse and expiry

## 3. Sync endpoint and ACL

- [ ] 3.1 `GET /sync` upgrade, ticket validation, run server-side `WsJsTransport` listener with `PeerId = user id + nonce`
- [ ] 3.2 `AclPolicy` implementing `AccessPolicy` from the `acl` table; `doc-unavailable` for foreign ids; test cross-user request
- [ ] 3.3 New-document acceptance rule: accept only ids listed in the user's index after sync, create owner ACL row; reject and close on unlisted push; tests
- [ ] 3.4 Lazy load + idle eviction with flush; restart persistence test
- [ ] 3.5 Interop test: stock `@automerge/automerge-repo` node client (websocket adapter) logs in, gets a ticket, syncs the workspace doc both ways

## 4. HTTP and hosting

- [ ] 4.1 `GET /api/health`, `GET /api/export` using core export schema scoped by ACL
- [ ] 4.2 Static `--web-dir` with SPA fallback
- [ ] 4.3 `serve` flags, TLS via rustls, `--insecure-http` guard, `contrib/tt-server.service`, `docs/server.md`

## 5. CLI integration

- [ ] 5.1 Update `tt login` to store `user.id`, add `tt logout`, daemon obtains tickets per connection, handles 401 by stopping sync and reporting in `tt status`
- [ ] 5.2 End-to-end test: server in temp dir, two users, daemon A syncs, daemon B cannot see A's docs, A's second daemon bootstraps from index id

## 6. Verification

- [ ] 6.1 clippy, fmt, `cargo test --workspace` green; security checklist in `docs/server.md` (TLS, argon2 params, rate limit, ticket flow)
