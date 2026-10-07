## ADDED Requirements

### Requirement: Day, week and month views
The timeline SHALL offer day, week and month views with previous/next/today navigation, a jump-to-date popover with calendar and natural-language input, and keyboard shortcuts per the design.

#### Scenario: Month to day
- **WHEN** the user clicks a day cell in month view
- **THEN** day view opens on that date

### Requirement: Overlapping entries render in lanes
Entries that overlap in time SHALL render side by side in equal lanes with a 3 px gutter; running entries SHALL extend to now with a live edge; two running entries SHALL be visually distinct.

#### Scenario: Three-way overlap
- **WHEN** three entries overlap
- **THEN** each occupies one third of the column width within the overlap cluster

### Requirement: Running strip
A persistent strip SHALL list every running entry with task title, elapsed time ticking each second, a stop button, and "Stop all".

#### Scenario: Stop all
- **WHEN** two entries are running and the user clicks Stop all
- **THEN** both receive the same end time and the strip empties

### Requirement: Drag interactions
Using dnd-kit the timeline SHALL support: drag on empty space to create (task picker on release), drag body to move, drag top/bottom edge (8 px handles) to resize, drop onto the task side panel to re-link, snapping to the configured grid with Alt disabling snap and Shift locking the column in week view, and a 4 px drag threshold.

#### Scenario: Create with snap
- **WHEN** snap is 15 min and the user drags from 13:07 to 13:52 on empty space
- **THEN** the proposed entry is 13:00–14:00 and releasing opens the task picker anchored to it

#### Scenario: Re-link by drop
- **WHEN** an entry is dropped onto task #30 in the side panel
- **THEN** the entry's task becomes #30 and an undo toast appears

### Requirement: Entry sheet
Clicking an entry SHALL open a sheet with task picker, start, end (or "now" when running), ±15 min and round chips, note, split at time, move to task, duplicate, delete; all edits apply immediately and are undoable.

#### Scenario: Invalid end
- **WHEN** the user types an end before the start
- **THEN** the field shows an inline error and the entry is unchanged

### Requirement: Totals
Day and week views SHALL show summed, wall-clock and overlap durations for the visible range, and month cells SHALL show the day total with top tasks; all totals SHALL be computed from the same entries that are rendered.

#### Scenario: Overlap shown
- **WHEN** two entries overlap by 10 minutes
- **THEN** overlap shows 10 min and summed minus wall-clock equals 10 min

### Requirement: Undo and redo
Every timeline and task edit SHALL be undoable with Ctrl+Z and redoable with Ctrl+Shift+Z within the session.

#### Scenario: Undo delete
- **WHEN** an entry is deleted and the user presses Ctrl+Z
- **THEN** the entry is restored with the same id and fields
