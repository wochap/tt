# time-tracking Specification

## Purpose
TBD - created by syncing change core-and-daemon. Concurrent running entries, start/stop, entry editing, bulk edit, and time parsing.

## Requirements

### Requirement: Concurrent running entries
The system SHALL allow any number of entries with `end` unset at the same time, across any tasks, including the same task.

#### Scenario: Two timers then stop all
- **WHEN** the user starts task #20, 20 minutes later starts task #30, and 5 minutes later runs `tt stop --all`
- **THEN** both entries are closed at the same stop time with durations of 25 and 5 minutes

### Requirement: Start and stop
`tt start <task>` SHALL create a running entry for that task starting now (or at `--at <time>`). With no argument on a TTY it SHALL open a fuzzy picker over open tasks. `tt stop` SHALL stop the single running entry, require a task or entry id when several are running, and accept `--all`.

#### Scenario: Stop is ambiguous
- **WHEN** two entries are running and the user runs `tt stop` without arguments
- **THEN** the command lists both and exits with code 4 without stopping anything

#### Scenario: Picker selection
- **WHEN** the user runs `tt start` on a TTY, types part of a ticket id and presses Enter
- **THEN** an entry starts for the selected task and its id is printed

### Requirement: Modify, move, split, delete
Entries SHALL be editable: `mod` changes start, end, note or task; `move` changes task only; `split --at <time>` turns one entry into two adjacent entries on the same task; `rm` deletes. Start SHALL be strictly before end when both are set.

#### Scenario: Invalid range
- **WHEN** the user sets an end earlier than start
- **THEN** the change is rejected with exit code 4 and the entry is unchanged

#### Scenario: Split a running entry
- **WHEN** a running entry started at 13:00 is split at 13:30
- **THEN** the first part is 13:00–13:30 and the second part is running since 13:30

### Requirement: Bulk edit table
`tt time edit [range]` SHALL open `$EDITOR` on a plain-text table (one entry per line: id, task seq, start, end, note) and apply additions, modifications and deletions on save.

#### Scenario: Line removed
- **WHEN** the user deletes a line from the table and saves
- **THEN** that entry is deleted and an `entry.deleted` event is emitted

### Requirement: Time and range parsing
Commands SHALL accept absolute ISO timestamps, `HH:MM` (today), `yesterday HH:MM`, relative offsets like `-30m`, and `now`; ranges SHALL accept `today`, `yesterday`, `week`, `lastweek`, `month`, `lastmonth`, and `<from>..<to>` with optional times. All are interpreted in the configured time zone and stored as UTC.

#### Scenario: Relative start
- **WHEN** the user runs `tt start 20 --at -15m`
- **THEN** the entry's start is 15 minutes before the current time
