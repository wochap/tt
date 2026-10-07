## Context

Greenfield Rust workspace. The hard parts are (1) offline-first replication that a browser can join with stock automerge-repo, (2) a local process model that gives hooks and a push event stream without polling, (3) a domain model where time entries never copy task data. A working Automerge repository core already exists in fi (`crates/automerge_repo`, Automerge 0.11, Tokio actors, `StorageAdapter` + `ControlStore` + `NetworkTransport` ports, own `FIRP` wire protocol with a bootstrap/root handshake). fi is read-only; we fork.

## Goals / Non-Goals

**Goals:**
- Single binary `tt` with subcommands `daemon`, `watch`, and all user commands.
- Works with no server configured. Sync is additive.
- Browser-compatible sync protocol from day one, proven by a spike before anything depends on it.
- Every write produces a domain event delivered to hooks and watchers, including writes that arrive from sync.
- Scriptable: `-j` on every read command, stable exit codes, no interactive prompts unless a TTY and no `-j`.

**Non-Goals:**
- HTTP server, auth backend, ACL enforcement (change `server`).
- Web UI (change `web`).
- Multi-user data in one local daemon. One daemon = one logged-in user.
- Project sharing between users (ACL port is designed for it, not implemented).

## Decisions

### D1. Fork fi's automerge_repo, generalize, keep its actor model
Copy `fi/crates/automerge_repo` into `crates/automerge_repo`. Keep: per-document Tokio actor, `DocHandle::{read,change,subscribe}`, `StorageAdapter`, `ControlStore`, `NetworkTransport`, flush/shutdown semantics, tests. Remove: `FIRP` protocol module, `Hello`/`BootstrapState`/`Inventory`/`Announce` bootstrap state machine, `initialize_new`/`join_existing` root semantics, quarantine/recovery that depend on them (keep what is protocol-agnostic). Add: `AccessPolicy` port (`fn may_sync(peer, doc) -> bool`, `fn visible_docs(peer) -> Vec<DocumentId>`), consulted before announcing or servicing any document for a peer. Default policy: allow all (local daemon). Alternative considered: `automerge-repo-rs` crate; rejected, less mature than the fi fork and would still need the policy port.

### D2. Wire protocol = automerge-repo JS protocol
Messages are CBOR maps: `join {senderId, peerMetadata, supportedProtocolVersions:["1"]}`, `peer {senderId, targetId, selectedProtocolVersion:"1", peerMetadata}`, `sync {documentId, data:bytes}`, `request {documentId, data}`, `unavailable`/`doc-unavailable {documentId}`, `ephemeral`, `remote-heads-changed`, `error`. Document id on the wire is bs58check of the 16 uuid bytes; `DocumentId(Uuid)` converts losslessly. Implement `crates/automerge_repo/src/transport/ws_js.rs` as a `NetworkTransport` over `tokio-tungstenite` (client mode now; server mode is a listener that spawns the same codec, used by the `server` change). The spike (task group 2) must pass before any domain work starts: a Rust client creates a doc, pushes it through a real node sync server, a second Rust client fetches it by id, changes it, and the first client observes the change. Test harness starts node itself; if node or the npm package is missing the test is skipped with a loud message, never silently green.

### D3. Document sharding
- `workspace` doc per user: `{ user:{id,name}, projects:{id→{name,color,archived}}, tags:{id→{name,color}}, tasks:{id→{seq,title,description,tags:[tagId],project?,metadata:{k:v},state,created,updated}}, counters:{taskSeq} }`.
- `entries-YYYY` doc per calendar year keyed by the entry's start date in UTC: `{ entries:{id→{task,start,end?,note?,created,updated}} }`.
- `index` doc per user listing workspace and entries doc ids, created on first run, its id stored in config. Index id is what the server returns at login so a fresh device can join.
- `seq` allocation: `counters.taskSeq` incremented inside the same change as task creation. Two offline devices can collide; on merge Automerge keeps one counter value and both tasks keep their `seq`. Collision resolution: daemon detects duplicate `seq` after sync and reassigns the newer task (by `created`) the next free number, emitting `task.updated`. Acceptable for one person with few devices.
- Alternative: one doc for everything. Rejected, load time grows unbounded.

### D4. Daemon is the single writer and broker
`tt daemon` opens the Repo with `SqliteStorage` (`documents(key TEXT PRIMARY KEY, bytes BLOB)` plus `control` table; atomic replace per key, as the port demands). Unix socket at `$XDG_RUNTIME_DIR/tt.sock`, newline-delimited JSON-RPC 2.0 frames. CLI auto-spawns the daemon (detached, same binary, `tt daemon --spawned`) if the socket is absent or stale, waits for readiness. A `systemd --user` unit is shipped under `contrib/`. Daemon never idle-exits. Multi-process over the same SQLite is forbidden by design; the daemon takes an exclusive `flock` on the db path and refuses to start twice.

### D5. Domain events derived by diffing, not by command
After every document change (local or from sync) the daemon diffs the previous and new domain view of the affected doc and emits typed events: `task.created|updated|deleted`, `project.*`, `tag.*`, `entry.started` (new entry with `end == null`), `entry.stopped` (`end` transitioned null→value), `entry.updated`, `entry.moved` (task changed), `entry.deleted`. Payload always includes the full entry, its task, task's project and tags, the current list of all running entries, `origin: local|remote`, and a monotonic `seq`. Deriving from diffs means remote changes produce identical events to local ones. Event log is kept in memory (ring, last 10k) and watchers reconnecting pass `since` to replay.

### D6. Hooks
Directory `$XDG_CONFIG_HOME/tt/hooks/`. Files named `<event>` or `<event>.d/*` (e.g. `entry.started`, `all.d/notify.sh`). Daemon forks each executable with env `TT_EVENT`, `TT_ORIGIN`, and the JSON payload on stdin, with a 10 s timeout, output logged. Hooks never block the write; they run after commit. Filtering is the script's job.

### D7. `tt watch`
Connects to the daemon, subscribes with optional filters (`--task`, `--tag`, `--project`, `--event`), prints one JSON object per line. First line is a `snapshot` event with all running entries so a widget needs no state. Quickshell runs it as a `Process` and parses lines.

### D8. CLI surface
```
tt task add "<title>" [+tag ...] [@project] [key:value ...]
tt task ls [--state open|done|archived|all] [--tag T] [--project P] [-j]
tt task show <seq|uuid> | edit <id> (opens $EDITOR, markdown with YAML frontmatter) | mod <id> --title/--add-tag/--rm-tag/--project/--meta k=v/--state | done <id> | rm <id>
tt start [<id>|<fuzzy text>]   (no arg and TTY: picker over open tasks, title + metadata values + tags)
tt stop [<entry-id>|<task-id>|--all]
tt time ls [range] | mod <id> --start/--end/--task/--note | move <id> --task <id> | split <id> --at <time> | rm <id> | edit [range] (editable table in $EDITOR)
tt report [day|week|month|<from>..<to>] [--by task|tag|project] [-j]
tt export [--format json|csv] [--round 15m --mode up|nearest --group entry|task-day] [range]
tt import <file.json>
tt watch [filters] ; tt daemon ; tt login <server> ; tt config get|set ; tt project ... ; tt tag ...
```
Time parsing accepts ISO, `13:00`, `yesterday 13:00`, `-30m`, `now`. Ranges: `today|yesterday|week|lastweek|month|lastmonth|YYYY-MM-DD..YYYY-MM-DD[THH:MM]`.
Exit codes: 0 ok, 1 usage, 2 not found, 3 daemon unreachable, 4 conflict/invalid state.

### D9. Reports and rounding
`summed` = Σ durations. `wall` = length of the union of intervals. Both always printed. Rounding/flatten algorithm lives in `tt-core` as a pure function with property tests: output intervals never overlap, each has duration ≥ input duration rounded per mode, order by original start is preserved. `per-task-day` grouping sums durations per (task, local day) before rounding and emits one interval per group starting at the group's first start.

### D10. Config and login
`$XDG_CONFIG_HOME/tt/config.toml`: `server.url`, `server.token`, `user.index_doc`, `week_start`, `snap`, `tz`, `editor` override. `tt login <url>` prompts username/password, POSTs to `/api/login` (contract defined now, implemented in `server` change), stores token and index doc id, tells the daemon to reconnect. Without `server.url` the daemon runs purely local.

## Risks / Trade-offs

- [JS protocol details drift between automerge-repo versions] → pin the npm version in the spike harness, assert `selectedProtocolVersion == "1"`, keep the codec in one file with fixture round-trip tests.
- [Daemon not running when hooks expected] → CLI auto-spawn plus systemd unit; `tt status` reports daemon state.
- [Large entries doc after years] → per-year sharding; old years are opened lazily only for reports touching them.
- [seq collisions across devices] → repair step in D3, covered by a test.
- [Editor round-trip corrupts frontmatter] → parse strictly, on parse error keep the temp file and print its path, never write partial data.
- [Hook storms] → events coalesced per document change, not per field.

## Open Questions

- None blocking. Node availability on CI for the spike is an environment question; the test skips loudly if absent.
