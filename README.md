# tt

Offline-first task and time tracker. A scriptable CLI, a daemon that runs
hooks and streams events, a self-hosted sync server, and a web app (PWA).
Every device keeps a full copy of your data in Automerge documents and syncs
when it can.

- Concurrent timers, tasks with projects, tags and metadata
- `tt watch` event stream and hook scripts for status bars and automation
- Reports, rounding, JSON/CSV export and import
- Multi-user server with per-user ACLs. The web app works offline.

## Architecture

```text
 tt (CLI) ──JSON-RPC──▶ tt daemon ──websocket /sync──▶ tt-server ◀──/sync── web app (browser)
  $XDG_RUNTIME_DIR/tt.sock   │  Automerge repo on disk      │ SQLite: users, tokens,     IndexedDB +
                             │  events ▶ hooks, tt watch   │ ACL, documents             service worker
```

| Path | What |
| --- | --- |
| `crates/tt-core` | Domain model over Automerge documents: tasks, entries, events, reports, rounding |
| `crates/tt-daemon` | Owns the repo, serves socket RPC, derives events, runs hooks, syncs |
| `crates/tt-cli` | `tt` binary: the CLI, plus `tt daemon` |
| `crates/tt-server` | `tt-server`: accounts, tokens, ACL, automerge-repo websocket sync, web hosting |
| `crates/automerge_repo` | Automerge repository speaking the automerge-repo JS wire protocol |
| `packages/domain` | TypeScript domain model shared with the web app |
| `web/` | React + Vite web app, a full offline peer of the server |

The CLI talks to a local daemon (started on demand, or as a user service).
The daemon syncs with `tt-server` over the same protocol the browser uses,
so CLI, other devices and the web app all converge.

## Docs

- [docs/cli.md](docs/cli.md): command reference
- [docs/events.md](docs/events.md): event types, payload, `tt watch`, socket protocol
- [docs/hooks.md](docs/hooks.md): hook scripts and a Quickshell status widget
- [docs/server.md](docs/server.md): server commands, HTTP API, sync, security
- [docs/web.md](docs/web.md): web app build and architecture
- [docs/export.md](docs/export.md): document schema and export format

## Install

With Nix:

```sh
nix run github:wochap/tt -- status       # or: nix profile install .#tt
nix build .#tt-web                       # static web bundle for --web-dir
```

From source: `cargo build --release -p tt-cli -p tt-server` (binaries in
`target/release`) and `pnpm install && pnpm build` for the web app (`web/dist`).

## Usage

```sh
tt task add "Fix login" +backend @web ticket:PROJ-123
tt start 1          # or fuzzy text, or a picker with no argument
tt status
tt stop
tt report
```

Local data lives in `$XDG_DATA_HOME/tt`, config in `$XDG_CONFIG_HOME/tt/config.toml`.
`contrib/tt.service` is a systemd user unit for the daemon.

## Production

```sh
tt-server --db /var/lib/tt-server/server.db user add alice       # no public sign-up
tt-server --db /var/lib/tt-server/server.db serve \
    --tls-cert fullchain.pem --tls-key privkey.pem --web-dir /path/to/tt-web
# or behind a TLS reverse proxy on the same host:
#   serve --insecure-http --behind-proxy --listen 127.0.0.1:8080

tt login https://tt.example.com --username alice                # on each device
```

Clients verify certificates against the public webpki roots, so use a
publicly trusted certificate (or plain `http://` on loopback).
`contrib/tt-server.service` is a hardened system unit. Details:
[docs/server.md](docs/server.md).

## Development

```sh
nix develop          # rust toolchain, node, pnpm, just, sqlite
just build           # cargo build --workspace
just test            # cargo test --workspace
just lint            # clippy -D warnings
just server          # tt-server on 127.0.0.1:8080, plain HTTP
pnpm --filter web dev  # vite on :5173, proxies /api and /sync
just web             # web lint, typecheck, vitest, build
just ci              # everything, including interop and e2e
```

Specs and change proposals live in `openspec/`.

## License

[MIT](LICENSE)
