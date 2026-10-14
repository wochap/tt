# web-reports-export Specification

## Purpose
TBD - created by syncing change web. Range reports, grouping, JSON/CSV download, rounded + flattened preview with per-entry and per-task-day grouping.

## Requirements

### Requirement: Range reports
Reports SHALL accept day, week, month, or a custom range with optional start and end times, and show summed, wall-clock and entry count plus grouping by task, tag and project.

#### Scenario: Custom range with times
- **WHEN** the range is 2026-10-01 08:00 to 2026-10-03 18:00
- **THEN** only entries overlapping that window are counted, clipped to it

### Requirement: Export panel
Export SHALL offer raw JSON, CSV, and a rounded + flattened variant with a duration grid (5/10/15/30/60) and mode up|nearest, a start grid (off/5/10/15/30/60) and mode up|nearest, and grouping per-entry | per-task-day, showing raw and flattened strips and tables side by side with moved/snapped/rounded/dropped badges before download. Start grids that do not divide the chosen duration grid SHALL be disabled, and changing the duration grid so that the current start grid no longer divides it SHALL set the start grid to the largest valid value. A row SHALL be badged moved when it was pushed later than its snapped start, snapped when its start differs from its raw start without being pushed, rounded when its duration differs from its raw duration, and dropped when its rounded duration is 0. Dropped rows SHALL stay in the preview opposite their raw row and SHALL NOT be included in the downloaded JSON or CSV.

#### Scenario: Preview equals domain function
- **WHEN** the preview renders for a range
- **THEN** its intervals equal the output of the shared round+flatten function for the same input and options

#### Scenario: Brief example
- **WHEN** entries are 13:00–13:05 and 13:10–13:45 with duration 15 min up, start off, per-entry
- **THEN** the preview shows 13:00–13:15 and 13:15–14:00

#### Scenario: Snapped start
- **WHEN** an entry is 21:22–21:37 with duration 15 min nearest and start 15 min nearest
- **THEN** the rounded row reads 21:15–21:30 with a snapped badge

#### Scenario: Invalid start grid disabled
- **WHEN** the duration grid is 15 min
- **THEN** the start grid options 10, 30 and 60 min are disabled

#### Scenario: Start grid falls back
- **WHEN** the start grid is 15 min and the user changes the duration grid to 10 min
- **THEN** the start grid becomes 10 min

#### Scenario: Dropped row not downloaded
- **WHEN** an entry of 4 minutes is rounded nearest to a 15 min duration grid
- **THEN** the preview shows it with a dropped badge and the downloaded CSV has no row for it
