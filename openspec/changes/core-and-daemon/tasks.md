## 1. Workspace scaffold

- [ ] 1.1 Create Cargo workspace with crates `automerge_repo`, `tt-core`, `tt-daemon`, `tt-cli` (binary `tt`), edition 2024, pinned deps (automerge =0.11.0, tokio, rusqlite bundled, serde, clap, chrono, ciborium, bs58, tokio-tungstenite)
- [ ] 1.2 Copy `fi/crates/automerge_repo` into `crates/automerge_repo` (copy only; verify `git -C ~/Sandboxes/sandbox/fi status` is clean before and after), make it build in this workspace
- [ ] 1.3 Add `rtk`-prefixed `justfile` or `scripts/` targets: build, test, lint (clippy -D warnings), fmt check

## 2. Spike: automerge-repo JS wire protocol (gate for everything after)

- [ ] 2.1 Implement CBOR codec for `join`, `peer`, `sync`, `request`, `unavailable`, `doc-unavailable`, `ephemeral`, `error` with fixture round-trip tests; bs58check conversion for `DocumentId`
- [ ] 2.2 Implement `WsJsTransport` (client) as `NetworkTransport` over tokio-tungstenite with handshake, reconnect with backoff, typed error on protocol version mismatch
- [ ] 2.3 Test harness that installs/starts `@automerge/automerge-repo-sync-server` (pinned version, `npm` into a scratch dir) on a free port, skips loudly if node missing
- [ ] 2.4 Integration test: client A creates doc, client B fetches by id, B changes, A observes within 5 s; run green in CI script
- [ ] 2.5 Remove `FIRP` protocol, bootstrap state machine, root/join semantics from the fork; keep port tests passing; document the fork delta in `crates/automerge_repo/FORK.md`

## 3. Repository generalization

- [ ] 3.1 Add `AccessPolicy` port with allow-all default; consult it on announce, request, sync paths; test deny path returns `doc-unavailable`
- [ ] 3.2 Implement `SqliteStorage` (`StorageAdapter` + `ControlStore`) with atomic key replace and crash test
- [ ] 3.3 Server-side listener variant of the websocket codec (accept connections, run same state machine) so the `server` change only adds auth and policy

## 4. Domain core (`tt-core`)

- [ ] 4.1 Define document schemas (index, workspace, entries-YYYY) and typed read/write helpers over Automerge; migrations stub with schema version field
- [ ] 4.2 Projects, tags, tasks CRUD with `seq` counter, state, metadata; seq collision repair after sync with test
- [ ] 4.3 Entries CRUD: start (concurrent), stop (single/ambiguous/all), mod, move, split, delete, validation start<end
- [ ] 4.4 Time and range parser (ISO, HH:MM, yesterday, -30m, now, named ranges, from..to) with tz handling and tests
- [ ] 4.5 Domain diff: previous vs new view → typed events with full payload and running list; tests for local and remote origin
- [ ] 4.6 Reports: grouping by task/tag/project/day, summed and wall totals (interval union)
- [ ] 4.7 Rounding + flatten pure function with modes up|nearest, grouping entry|task-day, property tests (no overlap, order kept, duration bound) and the brief's example
- [ ] 4.8 Export/import JSON schema (versioned) and CSV writer; round-trip test
- [ ] 4.9 Fuzzy search over title, tags, metadata values (nucleo or skim matcher), ordered by score

## 5. Daemon (`tt-daemon`)

- [ ] 5.1 Startup: XDG paths, exclusive flock, open Repo with SqliteStorage, load or create index + workspace docs, lazy-open entries docs
- [ ] 5.2 Unix socket JSON-RPC 2.0 server with one method per command; stale socket detection
- [ ] 5.3 Event bus: in-memory ring (10k), seq, subscription method with filters and `since` replay, `snapshot` first, `gap` marker
- [ ] 5.4 Hook runner: discover `hooks/<event>`, `<event>.d/*`, `all.d/*`, fork with stdin JSON + env, 10 s timeout, logging, never blocks write
- [ ] 5.5 Optional sync: when `server.url` + token present, connect `WsJsTransport` with bearer header, reconnect loop, status exposed via RPC; no network when unset
- [ ] 5.6 `contrib/tt.service` systemd user unit and daemon log file under `$XDG_STATE_HOME/tt/`

## 6. CLI (`tt`)

- [ ] 6.1 clap command tree per design D8, global `-j`, exit code mapping, daemon auto-spawn + readiness wait
- [ ] 6.2 `task add|ls|show|mod|done|rm|find`, `project`, `tag` commands with quick-create syntax
- [ ] 6.3 `task edit`: markdown + YAML frontmatter round trip through `$EDITOR`, strict parse, keep temp file on error
- [ ] 6.4 `start` (id, fuzzy text, or TTY picker), `stop` (single, by id, `--all`, ambiguity error)
- [ ] 6.5 `time ls|mod|move|split|rm` and `time edit` editable table with diff-apply
- [ ] 6.6 `report`, `export` (json/csv/rounded), `import`
- [ ] 6.7 `watch` with filters printing NDJSON; `status` showing daemon, sync and running entries
- [ ] 6.8 `config get|set`, `login <url>` (prompt, POST, store token 0600, notify daemon)

## 7. Verification and docs

- [ ] 7.1 End-to-end test script: spawn daemon in temp XDG dirs, run the brief's flows (create, fuzzy start, concurrent start/stop, rename propagates, edit time, report, rounded export, hook fired, watch stream) and assert outputs
- [ ] 7.2 `docs/cli.md` generated from clap, `docs/events.md` payload schema, `docs/hooks.md` with a quickshell example
- [ ] 7.3 clippy clean, fmt clean, `cargo test --workspace` green
