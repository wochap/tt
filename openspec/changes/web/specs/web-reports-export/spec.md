## ADDED Requirements

### Requirement: Range reports
Reports SHALL accept day, week, month, or a custom range with optional start and end times, and show summed, wall-clock and entry count plus grouping by task, tag and project.

#### Scenario: Custom range with times
- **WHEN** the range is 2026-10-01 08:00 to 2026-10-03 18:00
- **THEN** only entries overlapping that window are counted, clipped to it

### Requirement: Export panel
Export SHALL offer raw JSON, CSV, and a rounded + flattened variant with grid (5/10/15/30/60), mode up|nearest and grouping per-entry | per-task-day, showing raw and flattened strips and tables side by side with moved/rounded badges before download.

#### Scenario: Preview equals domain function
- **WHEN** the preview renders for a range
- **THEN** its intervals equal the output of the shared round+flatten function for the same input and options

#### Scenario: Brief example
- **WHEN** entries are 13:00–13:05 and 13:10–13:45 with 15 min, up, per-entry
- **THEN** the preview shows 13:00–13:15 and 13:15–14:00
