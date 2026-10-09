# event-stream-and-hooks Specification

## Purpose
TBD - created by syncing change core-and-daemon. Domain events derived from document changes, the watch stream, and hook scripts.

## Requirements

### Requirement: Domain events derived from document changes
After every document change, local or received through sync, the daemon SHALL emit typed events (`task.created|updated|deleted|renumbered`, `project.*`, `tag.*`, `entry.started|stopped|updated|moved|deleted`) each carrying a monotonic `seq`, `origin` (`local` or `remote`), the full affected record, the related task with its project and tags, and the list of all currently running entries. `task.renumbered` SHALL additionally carry `from` and `to` short ids, and SHALL be emitted instead of `task.updated` when the only change to a task is its renumbering.

#### Scenario: Remote stop
- **WHEN** another device stops an entry and the change arrives through sync
- **THEN** the daemon emits `entry.stopped` with `origin: remote` and the same payload shape as a local stop

#### Scenario: Renumber after sync
- **WHEN** a seq collision is repaired after sync and task "Fix login" moves from #20 to #31
- **THEN** the daemon emits one `task.renumbered` event with `from: 20`, `to: 31` and the task record, and runs `task.renumbered` hooks

### Requirement: Watch stream
`tt watch` SHALL print one JSON object per line, starting with a `snapshot` event listing all running entries, then every matching event as it happens, and SHALL support filters `--event`, `--task`, `--tag`, `--project` and `--since <seq>` for replay.

#### Scenario: Widget restart
- **WHEN** a watcher reconnects with `--since 41`
- **THEN** it receives events 42 onward from the in-memory log, or a `snapshot` plus a `gap` marker if 42 was evicted

#### Scenario: Tag filter
- **WHEN** `tt watch --tag work` is running and an entry starts on a task without the `work` tag
- **THEN** no line is printed for that event

### Requirement: Hook scripts
The daemon SHALL execute every executable in `$XDG_CONFIG_HOME/tt/hooks/<event>` and `<event>.d/`, and in `all.d/`, after each event, passing the event JSON on stdin and `TT_EVENT`, `TT_ORIGIN`, `TT_SEQ` in the environment, with a 10 second timeout, without blocking the originating command.

#### Scenario: Hook failure
- **WHEN** a hook exits non-zero or times out
- **THEN** the originating write is already committed, the failure is logged by the daemon, and other hooks still run
