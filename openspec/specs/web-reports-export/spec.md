# web-reports-export Specification

## Purpose
TBD - created by syncing change web. Range reports, grouping, JSON/CSV download, rounded + flattened preview with per-entry and per-task-day grouping.

## Requirements

### Requirement: Range reports
Reports SHALL accept day, week, month, or a custom range with optional start and end times, and show summed, wall-clock and entry count plus grouping by task, tag and project. Reports SHALL offer the same Filter control as the timeline (button with count, popover with search, Projects incl. "No project" and Tags with color and tracked time in the range, removable chips with Clear, `F` to open and `⇧F` to clear) with the same matching rule. Entries that do not match SHALL be excluded from the totals, the task/tag/project groupings, the export preview and every download (raw JSON, raw CSV and rounded). While a filter is active the totals SHALL read "Filtered · <filtered summed> of <all summed>" and "Entries <filtered> of <all>". When nothing matches, the report SHALL show "No entries match these filters" with Clear filters and Edit filter actions and downloads SHALL be disabled. The filter SHALL be kept in the Reports page URL, independent of the timeline's filter, and SHALL persist across preset and range changes.

#### Scenario: Custom range with times
- **WHEN** the range is 2026-10-01 08:00 to 2026-10-03 18:00
- **THEN** only entries overlapping that window are counted, clipped to it

#### Scenario: Filtered totals
- **WHEN** the day has 7h 29m tracked, of which 3h 37m on Atlas tasks, and the user filters Reports to Atlas
- **THEN** the totals read "Filtered · 3h 37m of 7h 29m" and the task, tag and project panels list only Atlas entries

#### Scenario: Filter applies to downloads
- **WHEN** Reports is filtered to tag billing and the user downloads raw CSV and rounded CSV
- **THEN** both files contain only entries whose task carries billing

#### Scenario: Filtered entries do not push rounded rows
- **WHEN** a non-matching entry overlaps a matching one and Reports is filtered
- **THEN** the rounded preview and download are computed from matching entries only, so the matching entry is not moved by it

#### Scenario: Nothing matches
- **WHEN** the filter matches no entry in the range
- **THEN** the empty state "No entries match these filters" is shown and the download buttons are disabled

#### Scenario: Independent from the timeline
- **WHEN** the timeline is filtered to Atlas and the user opens Reports
- **THEN** Reports is unfiltered

### Requirement: Export panel
Export SHALL offer raw JSON, CSV, and a rounded + flattened variant with a Duration menu (grid 5/10/15/30/60 and mode up|nearest), a Start menu (grid off/5/10/15/30/60 and mode up|nearest), and grouping per-entry | per-task-day. The Duration and Start grids SHALL start from the Settings defaults and changes in Reports SHALL apply to Reports for the session only. Start grids that do not divide the chosen duration grid SHALL be disabled, and changing the duration grid so that the current start grid no longer divides it SHALL set the start grid to the largest valid value. A row SHALL be badged moved when it was pushed later than its snapped start, snapped when its start differs from its raw start without being pushed, rounded when its duration differs from its raw duration (snapped and rounded may both show), and dropped when its rounded duration is 0. A rounded row that ends on a later local day than the day it is listed under SHALL show a "+1d" mark. Dropped rows SHALL stay in the preview, dimmed, opposite their raw row, marked "not in download", and SHALL NOT be included in the downloaded JSON or CSV; the panel SHALL state "<n> rows in download" and, when any are dropped, "· <m> dropped".

In per-entry grouping the preview SHALL be one scrolling table in which each row pairs a raw entry (left) with its rounded row (right), so both halves always scroll together; hovering or focusing either half SHALL highlight both and SHALL reveal the raw time on the rounded half. In per task / day grouping the raw entries SHALL be listed left and the rounded task/day rows right, aligned per day section with the shorter side padded.

When the range spans more than one day, the preview SHALL group rows under a day header row (e.g. "Tue 20 Oct") spanning both halves with the raw subtotal left and the rounded subtotal right, and the header of the day being scrolled SHALL stay pinned at the top. When the range spans more than 7 days, a week subtotal row SHALL follow each week's days, with weeks following the week-start setting. A single-day range SHALL show no day header. Rows SHALL be listed under the local day of their raw start. Column headers SHALL keep the totals of the whole range. The day strip above the tables SHALL show the pinned day, or a day whose header was clicked, and SHALL keep its previous/next stepper and its "day ends later" label. On phone layouts each row SHALL stack the raw time, faint, above the rounded time, and day headers SHALL stay sticky.

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

#### Scenario: Defaults from Settings
- **WHEN** Settings has start 5 min and duration 30 min and the user opens Reports
- **THEN** the export controls read Duration 30 min and Start 5 min

#### Scenario: Dropped row not downloaded
- **WHEN** an entry of 4 minutes is rounded nearest to a 15 min duration grid
- **THEN** the preview shows it dimmed with a dropped badge and "not in download", the note counts it as dropped, and the downloaded CSV has no row for it

#### Scenario: Pushed past midnight
- **WHEN** Tue 23:55–00:20 is rounded with duration and start 15 min nearest in a week range
- **THEN** the row is listed under Tue, reads 00:00–00:30 with a "+1d" mark, and the next day's early rows are pushed after it

#### Scenario: Paired rows scroll together
- **WHEN** a week range in per-entry grouping is scrolled
- **THEN** each raw entry stays on the same row as its rounded row

#### Scenario: Sticky day header
- **WHEN** the user scrolls a week range past Monday's rows into Tuesday's
- **THEN** the Tuesday header with its raw and rounded subtotals is pinned at the top of the table and the day strip shows Tuesday

#### Scenario: Week subtotals in a month
- **WHEN** the range is a month
- **THEN** after each week's last day a week row shows that week's raw and rounded subtotals

#### Scenario: Single day
- **WHEN** the range is one day
- **THEN** no day header or week row is shown

#### Scenario: Click a day header
- **WHEN** the user clicks the Thursday header
- **THEN** the day strip shows Thursday
