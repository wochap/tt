## Why

Taskwarrior + timewarrior split task identity from tracked time, break on concurrent timers, and expose weak hooks, so the author's daily flow (create task with a ticket id, fuzzy-find it, start, stop, fix times, report monthly) is fragile. tt replaces both with one offline-first engine whose time entries reference tasks by id, allow concurrency, and emit events without polling.

## What Changes

- New Rust workspace with crates: `automerge_repo` (forked from fi, generalized), `tt-core` (domain model over Automerge docs, event derivation, reports, rounding), `tt-daemon`, `tt-cli`.
- Spike: a Rust `NetworkTransport` that speaks the automerge-repo JS wire protocol, verified end to end against the real `@automerge/automerge-repo-sync-server` running under node, with no human in the loop.
- `tt daemon`: owns the Repo and a SQLite blob store, serves a unix socket RPC, derives domain events from document changes, forks hook scripts, streams events to watchers, and (when configured) syncs to a remote server over websocket.
- `tt` CLI: projects, tags, tasks (create, edit in `$EDITOR` as markdown + frontmatter, state, metadata), time entries (start, stop, modify, move, split, delete, bulk edit as an editable table), fuzzy picker, `-j` JSON output everywhere, reports by day/week/month/range, export/import JSON and CSV, config file with server endpoint and token.
- `tt watch`: long-lived NDJSON event stream with filters, for status bars.
- Local login flow stores a token obtained from the server (server itself is a later change); offline mode works with no server configured.

## Capabilities

### New Capabilities
- `sync-core`: forked repository crate with storage/transport ports, JS-protocol transport, access-policy port, document sharding (workspace per user, entries per year).
- `task-management`: projects, tags, tasks with title, markdown description, tags, metadata, state, stable `seq` id, editor-based editing, fuzzy search.
- `time-tracking`: concurrent running entries, start/stop/modify/move/split/delete, bulk edit table, time parsing.
- `event-stream-and-hooks`: domain event derivation, `tt watch` NDJSON stream with filters, hook scripts with full payload.
- `reports-and-export`: summaries by day/week/month/range with summed and wall-clock totals, rounded + flattened export, JSON/CSV export and import.
- `cli-and-config`: daemon lifecycle and socket RPC, config file, login/token storage, JSON output contract, exit codes.

### Modified Capabilities
(none, greenfield)

## Impact

- New repository layout: `Cargo.toml` workspace, `crates/*`, `docs/`.
- External deps: automerge 0.11, tokio, rusqlite (bundled), tokio-tungstenite, ciborium (CBOR), bs58, clap, serde, chrono, notify-free (no inotify needed, daemon is the broker), skim or nucleo for the picker.
- Dev dep for the spike: node + `@automerge/automerge-repo-sync-server` started by the test harness.
- fi project untouched.
