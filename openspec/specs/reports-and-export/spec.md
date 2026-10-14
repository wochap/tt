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
`tt export --round <grid>` SHALL round each item's duration to the duration grid (`--mode up` default, or `nearest`), snap each item's start to a start grid (`--start <grid|off>`, `--start-mode up|nearest`, mode defaulting to `--mode`), then lay items out in order of original start so that none overlap: each start = max(own snapped start, previous end), each end = start + rounded duration, with the previous end carried across the whole range rather than reset at midnight. Start snapping SHALL use the local wall clock of the export time zone, `up` SHALL round to the next grid mark and `nearest` SHALL round to the closest mark with halves going up; `off` SHALL keep raw starts. The start grid SHALL divide the duration grid; any other pair SHALL be rejected with an error naming the valid start grids. When `--start` is not given, the start grid SHALL be the config value `snap_start`, else the largest of 15, 10 and 5 minutes that divides the duration grid, else off. With `--group task-day`, durations SHALL be summed per task per local day before rounding and each group SHALL start at its first entry's start, snapped. Every output item SHALL carry its local day, the local day of its raw start. An item whose rounded duration is 0 SHALL be dropped: it SHALL NOT move the previous end and SHALL NOT appear in the JSON or CSV output. The rounded JSON document SHALL be `tt-rounded` version 2 and SHALL include the duration grid, mode, start grid (0 for off), start mode and grouping.

#### Scenario: Example from the brief
- **WHEN** entries are 13:00–13:05 and 13:10–13:45 with `--round 15m --mode up --start off --group entry`
- **THEN** the output is 13:00–13:15 and 13:15–14:00

#### Scenario: Start snapped to nearest mark
- **WHEN** an entry is 21:22–21:37 with `--round 15m --mode nearest --start 15m`
- **THEN** the output is 21:15–21:30

#### Scenario: Start snapped up
- **WHEN** an entry is 21:22–21:37 with `--round 15m --mode up --start 15m`
- **THEN** the output is 21:30–21:45

#### Scenario: Large duration grid with a small start grid
- **WHEN** an entry is 09:05–09:20 with `--round 60m --mode up --start 15m --start-mode up`
- **THEN** the output is 09:15–10:15

#### Scenario: Snapped start pushed by the previous item
- **WHEN** entries are 09:00–09:55 and 09:50–10:05 with `--round 15m --mode nearest --start 15m`
- **THEN** the output is 09:00–10:00 and 10:00–10:15

#### Scenario: Push carries across midnight
- **WHEN** entries are Tue 23:55–00:20 and Wed 00:10–00:40 with `--round 15m --mode nearest --start 15m`
- **THEN** the output is Wed 00:00–00:30 with day Tuesday and Wed 00:30–01:00 with day Wednesday

#### Scenario: Zero-minute item dropped
- **WHEN** an entry is 21:07–21:11 with `--round 15m --mode nearest`
- **THEN** the JSON and CSV output contain no item for it and the next item is not pushed by it

#### Scenario: Local clock in a half-hour offset zone
- **WHEN** the time zone is Asia/Kolkata and an entry starts 09:20 local with `--round 60m --mode nearest --start 60m`
- **THEN** the output starts at 09:00 local

#### Scenario: Invalid grid pair
- **WHEN** `tt export --round 15m --start 10m` is run
- **THEN** the command fails with a usage error that lists off, 5m and 15m as valid start grids

#### Scenario: Default start grid
- **WHEN** `snap_start` is unset and `tt export --round 30m` is run without `--start`
- **THEN** starts are snapped to a 15 minute grid

#### Scenario: Per task per day
- **WHEN** a task has three 5 minute entries on one day starting 09:05, 13:10 and 17:20 with `--round 15m --mode up --start 15m --group task-day`
- **THEN** the output contains one 15 minute interval for that task on that day, 09:15–09:30

#### Scenario: Property
- **WHEN** any set of entries is exported with rounding and a valid grid pair
- **THEN** output intervals never overlap, their order by start equals the input order, each duration equals the input duration rounded per mode, and every start and end not from an `off` start grid lies on the start grid in local time

### Requirement: JSON and CSV export and import
`tt export --format json` SHALL dump projects, tags, tasks and entries for a range (or everything) in a documented schema; `--format csv` SHALL dump entries with task seq, title, project, tags, start, end, duration, note. `tt import <file.json>` SHALL load that schema, matching existing records by uuid.

#### Scenario: Round trip
- **WHEN** data is exported as JSON and imported into an empty daemon
- **THEN** `tt export --format json` on the second daemon is equal modulo ordering
