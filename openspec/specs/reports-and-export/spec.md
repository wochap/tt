# reports-and-export Specification

## Purpose
TBD - created by syncing change core-and-daemon. Summary reports, rounded/flattened export, and JSON/CSV export and import.

## Requirements

### Requirement: Summary reports
`tt report` SHALL accept a range and a grouping (`task`, `tag`, `project`, `day`) and print per-group durations plus `summed` (sum of durations) and `wall` (duration of the union of intervals) totals, with `-j` producing the same data as JSON.

#### Scenario: Overlap shows in totals
- **WHEN** two entries overlap by 10 minutes in the range
- **THEN** `summed` exceeds `wall` by exactly 10 minutes

### Requirement: Rounded and flattened export
`tt export --round <grid>` SHALL round each duration to the grid (`--mode up` default, or `nearest`), then lay entries out in order of original start so that none overlap (each start = max(own start, previous end)), optionally grouping per task per local day before rounding (`--group task-day`).

#### Scenario: Example from the brief
- **WHEN** entries are 13:00–13:05 and 13:10–13:45 with `--round 15m --mode up --group entry`
- **THEN** the output is 13:00–13:15 and 13:15–14:00

#### Scenario: Per task per day
- **WHEN** a task has three 5 minute entries on one day with `--round 15m --group task-day`
- **THEN** the output contains one 15 minute interval for that task on that day

#### Scenario: Property
- **WHEN** any set of entries is exported with rounding
- **THEN** output intervals never overlap, their order by start equals the input order, and each duration is at least the input duration rounded per mode

### Requirement: JSON and CSV export and import
`tt export --format json` SHALL dump projects, tags, tasks and entries for a range (or everything) in a documented schema; `--format csv` SHALL dump entries with task seq, title, project, tags, start, end, duration, note. `tt import <file.json>` SHALL load that schema, matching existing records by uuid.

#### Scenario: Round trip
- **WHEN** data is exported as JSON and imported into an empty daemon
- **THEN** `tt export --format json` on the second daemon is equal modulo ordering
