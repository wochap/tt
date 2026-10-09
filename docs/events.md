# tt events

The daemon derives events by diffing the domain view before and after every
document change, whether the change was a local command or arrived through
sync. Both paths produce the same payload; only `origin` differs. Events from
one change are emitted together, in this order: projects, tags, tasks,
entries.

Events reach two consumers:

- `tt watch` (NDJSON on stdout, see below)
- hook scripts (see [hooks.md](hooks.md))

## Types

| type | when |
|------|------|
| `project.created` / `project.updated` / `project.deleted` | project record added, changed, removed |
| `tag.created` / `tag.updated` / `tag.deleted` | tag record added, changed, removed |
| `task.created` / `task.updated` / `task.deleted` | task added, any field changed, removed |
| `task.renumbered` | a `seq` collision repair after sync moved the task to a new number (`from`, `to` are set); emitted instead of `task.updated`, which follows only if other fields changed too |
| `entry.started` | new entry with `end: null`, or a stopped entry made running again |
| `entry.created` | new entry that already has an end (time edit table, split, import) |
| `entry.stopped` | `end` went from `null` to a time |
| `entry.moved` | the entry's task changed (`previous_task` is set) |
| `entry.updated` | any other entry change (start, end, note) |
| `entry.deleted` | entry removed (`entry` is the last known state) |

## Payload

```json
{
  "seq": 42,
  "type": "entry.stopped",
  "origin": "local",
  "at": "2026-10-07T13:45:00.123Z",
  "entry": {
    "id": "6f1c…", "task_id": "01a1…",
    "start": "2026-10-07T13:00:00Z", "end": "2026-10-07T13:45:00Z",
    "duration": 2700, "running": false, "note": null,
    "created": "…", "updated": "…",
    "task": { "…": "TaskView, see below" }
  },
  "task": {
    "id": "01a1…", "seq": 12, "title": "Fix login", "description": "",
    "state": "open",
    "project": { "id": "…", "name": "web", "color": "#89b4fa" },
    "tags": [ { "id": "…", "name": "backend" } ],
    "metadata": { "ticket": "PROJ-123" },
    "created": "…", "updated": "…"
  },
  "running": [ { "…": "EntryView of every running entry after the change" } ]
}
```

Fields:

- `seq`: monotonic per daemon process, starting at 1. The last 10 000 events
  are kept in memory for replay.
- `origin`: `local` (this daemon's command) or `remote` (arrived through sync).
- `at`: when the daemon derived the event.
- `entry`: entry events only. Timestamps are UTC RFC 3339; `duration` is in
  seconds (running entries count up to `at`).
- `task`: the entry's task (entry events) or the task itself (task events),
  with project and tags resolved. For `task.deleted` it is the last known state.
- `previous_task`: `entry.moved` only.
- `from` / `to`: `task.renumbered` only, the old and new short ids. The task
  also lists every number it held before in `previous_seqs` (omitted when
  empty), so `tt task show <old>` can point to it.
- `project` / `tag`: project and tag events only, the full record.
- `running`: always present, every running entry after the change, ordered by
  start. A status bar can render from this field alone.

## `tt watch`

```
tt watch [--event TYPE]... [--task ID]... [--tag NAME]... [--project NAME]... [--since SEQ]
```

Prints one JSON object per line and flushes after each:

1. A `snapshot` first: `{"type":"snapshot","seq":<last seq>,"running":[…]}`
   (running entries filtered by `--task/--tag/--project`).
2. With `--since N`, every retained event after `N`. If `N + 1` was already
   evicted from the ring, a `{"type":"gap","from":N+1,"to":<oldest-1>}` marker
   comes first; rebuild state from the snapshot.
3. Then live events as they happen.

Filters: `--event` accepts exact types or prefixes ending in `*` (`entry.*`).
`--task` takes `12`, `#12` or a uuid; `--tag` and `--project` take names
(case-insensitive). Task-scoped filters drop project and tag events. Repeated
flags of one kind are OR-ed; different kinds are AND-ed.

If a watcher falls more than 4096 events behind, it gets a `gap` marker and
continues. `tt watch` exits with code 3 when the daemon goes away; run it
under a supervisor (Quickshell `Process` with restart) and pass `--since` with
the last seq you saw.

## Socket protocol

`tt watch` is a thin client over the daemon's JSON-RPC 2.0 socket
(`$XDG_RUNTIME_DIR/tt.sock`, newline-delimited). Other programs can subscribe
directly:

```json
{"jsonrpc":"2.0","id":1,"method":"subscribe","params":{"since":41,"events":["entry.*"],"tags":["work"]}}
```

The reply is `{"result":{"subscribed":true,"last_seq":…}}`, followed by
notifications `{"jsonrpc":"2.0","method":"event","params":<event>}`.
