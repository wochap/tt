## Why

The local daemon from `core-and-daemon` works alone, but the author wants to host tt for two people, sync between devices, and run the web GUI. That needs a server that authenticates users, enforces that one user never sees another's documents, and relays Automerge sync.

## What Changes

- New crate `tt-server` (binary `tt serve`, or `tt-server`): axum HTTP + websocket on one port.
- Password login issuing bearer tokens; user creation via CLI on the server host (`tt-server user add`), no public sign-up.
- Websocket endpoint speaking the automerge-repo JS protocol (server side of the transport built in `core-and-daemon`), with the authenticated user as `PeerId` and an `AccessPolicy` that only exposes documents owned by that user.
- Server-side Repo with SQLite storage holding every user's documents; per-user ACL table (owner now, shareable later).
- Static hosting of the web bundle from `web` change (directory configurable, served if present).
- `tt login` from the CLI now works end to end; server exposes a health endpoint and a JSON export endpoint per user for backups.

## Capabilities

### New Capabilities
- `server-auth`: users, password hashing, login, tokens, user admin CLI.
- `server-sync-acl`: websocket sync endpoint, peer identity from token, per-user document visibility, index document bootstrap.
- `server-api`: HTTP surface (login, me, export, health), static web hosting, configuration and deployment.

### Modified Capabilities
- `cli-and-config`: `tt login` contract gains the real response shape (`token`, `index_doc`, `user`) and token refresh/revocation behavior.

## Impact

- New crate depending on `automerge_repo`, `tt-core`, axum, tokio-tungstenite (server side), argon2, rusqlite.
- Deployment: single binary, one SQLite file, one port, reverse proxy optional. `contrib/tt-server.service`.
- Browser clients (next change) connect with the same protocol, so no server work is needed later.
