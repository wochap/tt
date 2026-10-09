# tt data formats

## Documents (Automerge)

Each user has three kinds of Automerge documents. Ids on the wire, in config,
and inside documents are automerge-repo bs58check document ids (the part after
`automerge:` in an automerge URL). Strings are Automerge `Text`, timestamps
are integer milliseconds since the Unix epoch (UTC), and every document has
`schema` (currently `1`) and `kind`.

```text
index      { schema, kind:"index", workspace:"<doc id>", entries:{ "2026":"<doc id>", … } }

workspace  { schema, kind:"workspace", user:{id,name},
             projects:{ <uuid>:{name, color?, archived, created, updated} },
             tags:    { <uuid>:{name, color?, created, updated} },
             tasks:   { <uuid>:{seq, title, description, tags:{<tagUuid>:true},
                                project?:<uuid>, metadata:{key:value},
                                state:"open"|"done"|"archived", created, updated} },
             counters:{taskSeq} }

entries-YYYY { schema, kind:"entries", year,
               entries:{ <uuid>:{task:<uuid>, start, end:null|ms, note?, created, updated} } }
```

- An entry lives in the document of the UTC year of its `start`, regardless
  of its end. Moving `start` into another year moves the entry.
- Task tags are a map used as a set so concurrent edits merge without
  duplicates.
- `seq` is allocated as `max(counters.taskSeq, max seq) + 1` in the same change
  that creates the task. When two offline devices allocate the same number,
  the daemon that sees the merge renumbers the later-created task (by
  `created`, then id), appends its old number to the task's `previous_seqs`
  list, and emits `task.renumbered`.
- Time entries refer to tasks by uuid only; renaming a task changes every
  report and listing at once.

## JSON export (`tt export --format json`)

```json
{
  "format": "tt-export",
  "version": 1,
  "exported_at": "2026-10-07T12:00:00Z",
  "range": { "from": "…", "to": "…" },
  "projects": [ { "id": "…", "name": "web", "color": "#89b4fa", "archived": false, "created": "…", "updated": "…" } ],
  "tags":     [ { "id": "…", "name": "backend", "created": "…", "updated": "…" } ],
  "tasks":    [ { "id": "…", "seq": 12, "title": "Fix login", "description": "",
                  "tags": ["<tag uuid>"], "project": "<project uuid>",
                  "metadata": { "ticket": "PROJ-123" }, "state": "open",
                  "created": "…", "updated": "…" } ],
  "entries":  [ { "id": "…", "task": "<task uuid>", "start": "…", "end": null,
                  "note": "…", "created": "…", "updated": "…" } ]
}
```

`range` is present only for ranged exports; then `entries` holds entries
overlapping the range while projects, tags and tasks are complete.

`tt import file.json` matches records by uuid: identical records are skipped,
others are written as in the file. An imported task whose `seq` belongs to a
different local task gets the next free number. Exporting from one daemon and
importing into an empty one yields an equal export, ignoring order and
`exported_at`.

## CSV (`--format csv`)

Columns: `id, task_seq, task_title, project, tags, start, end,
duration_seconds, note, task_id`. Tags are space-separated; times are UTC
RFC 3339; a running entry has an empty `end` and counts up to now.

## Rounded export (`--round 15m [--mode up|nearest] [--group entry|task-day]`)

Each duration is rounded to the grid (`up` by default). Items are then laid
out in order of original start so none overlap: `start = max(own start,
previous end)`, `end = start + rounded`. With `--group task-day`, durations
are summed per task per local day before rounding, starting at the group's
first start. Entries are clipped to the range first.

Example: 13:00–13:05 and 13:10–13:45, `--round 15m --mode up` →
13:00–13:15 and 13:15–14:00.

JSON output:

```json
{ "format": "tt-rounded", "version": 1, "grid_seconds": 900, "mode": "up", "group": "entry",
  "range": { "from": "…", "to": "…" },
  "items": [ { "task": { "…": "TaskView" }, "task_id": "…", "start": "…", "end": "…",
               "duration": 900, "original": 300, "entries": ["<entry uuid>"], "day": null } ],
  "total": 3600 }
```

CSV output uses the entry CSV columns; `id` lists the source entry ids.
