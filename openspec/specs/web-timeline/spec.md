# web-timeline Specification

## Purpose
TBD - created by syncing change web. Day/week/month views, lanes, running strip, entry sheet, drag interactions, undo/redo, totals.

## Requirements

### Requirement: Day, week and month views
The timeline SHALL offer day, week and month views with previous/next/today navigation, a jump-to-date popover with calendar and natural-language input, and keyboard shortcuts per the design. On phone layouts, the scrolling day timeline SHALL show a fade and a scroll indicator at its bottom edge while more content is below, and hide them when scrolled to the end.

#### Scenario: Month to day
- **WHEN** the user clicks a day cell in month view
- **THEN** day view opens on that date

#### Scenario: Phone day scroll affordance
- **WHEN** the phone day view has more hours below the visible area
- **THEN** a bottom fade and scroll indicator are shown, and they disappear once the user scrolls to the end

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
Using dnd-kit the timeline SHALL support: drag on empty space to create (task picker on release), drag body to move, drag top/bottom edge to resize, drop onto the task side panel to re-link, snapping to the configured grid with Alt disabling snap and Shift locking the column in week view, and a 4 px drag threshold. With a mouse or pen a drag SHALL begin once the pointer has moved past the threshold. With touch, a drag to create on empty space SHALL begin only after the finger has been held in place for a hold delay of about 250 ms; a touch that moves beyond a small tolerance before the hold completes SHALL scroll the timeline instead and SHALL NOT create anything. The dotted pending range left by a create drag SHALL itself be adjustable until a task is picked or it is dismissed: dragging its body SHALL move it and dragging its top or bottom edge SHALL resize it, with the same snapping, Alt and Shift behavior and one-grid-step minimum duration as entries, and a press on it SHALL NOT start a new create. The task picker SHALL follow the range to its new position after an adjust. Resize edges of the pending range and of entries SHALL have a touch-sized hit area (at least 40 px tall) without changing their visible appearance, while a block's body SHALL remain draggable even when the block is short.

#### Scenario: Create with snap
- **WHEN** snap is 15 min and the user drags from 13:07 to 13:52 on empty space
- **THEN** the proposed entry is 13:00–14:00 and releasing opens the task picker anchored to it

#### Scenario: Re-link by drop
- **WHEN** an entry is dropped onto task #30 in the side panel
- **THEN** the entry's task becomes #30 and an undo toast appears

#### Scenario: Move the pending range
- **WHEN** a pending range 13:00–14:00 is shown, snap is 15 min, and the user drags its body down by 30 min
- **THEN** the pending range becomes 13:30–14:30, no new range is created, and the task picker is anchored to the moved range

#### Scenario: Resize the pending range
- **WHEN** a pending range 13:00–14:00 is shown and the user drags its bottom edge to 14:50
- **THEN** the pending range becomes 13:00–15:00 and its start is unchanged

#### Scenario: Minimum duration while resizing
- **WHEN** snap is 15 min and the user drags the top edge of a pending range 13:00–14:00 below its bottom edge
- **THEN** the range is 13:45–14:00

#### Scenario: Empty space still creates
- **WHEN** a pending range is shown and the user drags on empty column space outside it
- **THEN** the old pending range is replaced by the newly drawn one

#### Scenario: Touch-sized edge
- **WHEN** a finger presses 15 px outside the visible bottom edge of a one-hour entry and drags down
- **THEN** the entry's end is resized rather than a new range being created

#### Scenario: Touch long-press then drag creates a range
- **WHEN** on a touch device the user presses empty space in the day column, holds still for the hold delay, then drags down 45 minutes' worth of grid and lifts
- **THEN** the proposed range spans the dragged interval snapped to the grid and the task picker opens anchored to it, exactly as for a mouse drag

#### Scenario: Quick swipe scrolls
- **WHEN** on a touch device the user swipes vertically on empty space in the day column before the hold delay elapses
- **THEN** the timeline scrolls, no range is proposed and no task picker opens

#### Scenario: Mouse behaviour unchanged
- **WHEN** the user presses the mouse on empty space and moves more than 4 px
- **THEN** the range is proposed immediately with no hold delay

### Requirement: Touch range creation feedback and scrolling
When a touch hold on empty space completes, the timeline SHALL signal that creation is armed with a visible cue on the pressed position and, where the device supports it, a short haptic pulse. From that moment until the finger lifts, the page SHALL NOT scroll, select text or show the long-press context menu because of that touch. While the finger is held within an edge zone at the top or bottom of the scrolling timeline, the timeline SHALL scroll in that direction and the proposed range SHALL keep following the finger. A tap that does not complete the hold SHALL behave as before and SHALL NOT arm creation.

#### Scenario: Armed cue
- **WHEN** a touch hold on empty space reaches the hold delay
- **THEN** a visible armed cue appears at the pressed time, a haptic pulse is emitted on devices that support it, and further finger movement resizes the proposed range instead of scrolling

#### Scenario: No context menu or selection
- **WHEN** the user long-presses empty space on an Android browser
- **THEN** no context menu or text-selection handles appear

#### Scenario: Edge auto-scroll while creating
- **WHEN** the user is dragging a new range with touch and holds the finger near the bottom edge of the scrolling timeline with more hours below
- **THEN** the timeline scrolls down and the range end keeps following the finger

#### Scenario: Tap does not arm
- **WHEN** the user lifts the finger before the hold delay
- **THEN** no armed cue or haptic pulse occurs and no drag is started

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
