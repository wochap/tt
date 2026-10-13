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
A persistent strip SHALL list every running entry with task title, elapsed time ticking each second, a stop button, and "Stop all". When nothing is running, the strip SHALL show the key hint "S start the focused task · N new entry at now" only on the timeline, where those keys are bound; on Tasks, task detail, Reports and Settings it SHALL show no tracking key hints.

#### Scenario: Stop all
- **WHEN** two entries are running and the user clicks Stop all
- **THEN** both receive the same end time and the strip empties

#### Scenario: Idle hint on the timeline
- **WHEN** nothing is running and the timeline is open
- **THEN** the strip shows "S start the focused task · N new entry at now"

#### Scenario: No tracking hint elsewhere
- **WHEN** nothing is running and Reports or Settings is open
- **THEN** the strip shows neither the S nor the N hint

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
Clicking an entry SHALL open a sheet with task picker, start, end (or "now" when running), ±15 min and round chips, note, split at time, move to task, duplicate, delete; all edits apply immediately and are undoable. On phone layouts, the sheet SHALL also offer "Select", which starts the phone entry selection with that entry selected.

#### Scenario: Invalid end
- **WHEN** the user types an end before the start
- **THEN** the field shows an inline error and the entry is unchanged

#### Scenario: Select from the sheet on phone
- **WHEN** the entry sheet is open on a phone layout
- **THEN** a "Select" action is shown, and on desktop it is not

### Requirement: Totals
Day and week views SHALL show summed, wall-clock and overlap durations for the visible range, and month cells SHALL show the day total with top tasks; all totals SHALL be computed from the same entries that are rendered. When a timeline filter is active, every total (the totals bar, the side panel, week day headers, the week and month footers and the month cells with their top tasks) SHALL count only the entries that match the filter, and the totals bar, side panel and footers SHALL mark the figures as filtered and show the unfiltered figure alongside, in the form "Filtered · 3h 37m of 7h 29m".

#### Scenario: Overlap shown
- **WHEN** two entries overlap by 10 minutes
- **THEN** overlap shows 10 min and summed minus wall-clock equals 10 min

#### Scenario: Filtered totals
- **WHEN** the day has 7h 29m tracked, of which 3h 37m on Atlas tasks, and the user filters to Atlas
- **THEN** the totals read "Filtered · 3h 37m of 7h 29m" and wall-clock and overlap are computed from the Atlas entries only

#### Scenario: Filtered month cells
- **WHEN** month view is filtered to tag meeting
- **THEN** each day cell shows only the time and top tasks of entries tagged meeting, and the month footer shows the filtered sum with the unfiltered sum alongside

### Requirement: Undo and redo
Every timeline and task edit SHALL be undoable with Ctrl+Z and redoable with Ctrl+Shift+Z within the session.

#### Scenario: Undo delete
- **WHEN** an entry is deleted and the user presses Ctrl+Z
- **THEN** the entry is restored with the same id and fields

### Requirement: Task picker closes on one Escape
When the task picker opened by a create drag is open, a single press of Escape SHALL close it and discard the pending range, whether focus is in its search input, on a result, or elsewhere in the picker. The search query SHALL NOT be cleared as a separate first step.

#### Scenario: Escape from the search input
- **WHEN** the user drags a range, the task picker opens with focus in "Search tasks, tags, ticket ids…", the user types "log" and presses Escape once
- **THEN** the picker is closed, the dotted pending range is gone, and no entry was created

#### Scenario: Escape with an empty query
- **WHEN** the picker has just opened with an empty query and the user presses Escape once
- **THEN** the picker is closed and the pending range is gone

### Requirement: Filter by project and tag
The timeline SHALL offer a Filter control in day, week and month views that opens a popover with a search field and two multi-select sections, Projects and Tags. Projects SHALL include a "No project" option. Each option SHALL show its color and the time tracked for it in the visible range; options with tracked time in the visible range SHALL be listed by default, and the search field SHALL find any project or tag of the workspace. An entry SHALL match the filter through its task: when projects are selected, the task's project SHALL be one of them ("No project" matches a task without a project and an entry whose task no longer exists); when tags are selected, the task SHALL carry at least one of them; when both sections have selections, both conditions SHALL hold. With no selection the timeline SHALL be unfiltered. Each active selection SHALL be shown as a removable chip near the toolbar, together with a Clear action and the note that other entries are dimmed and left out of totals. Entries that do not match SHALL remain at their position and size, and overlap lanes SHALL be computed from all entries as if unfiltered, but they SHALL be visibly dimmed and SHALL NOT be opened, dragged, resized, re-linked, focused with J/K, or selected. Creating a new entry by dragging on empty space SHALL work unchanged while a filter is active. `F` SHALL open the popover with the search field focused; inside it ↑/↓ SHALL move, Space SHALL toggle the highlighted option, Enter SHALL toggle it and close the popover, Backspace in an empty search SHALL remove the last selection, and Escape SHALL close it. `⇧F` SHALL clear all selections. The active selections SHALL be kept in the page URL, so they persist across switching between day, week and month and moving between dates, follow browser Back/Forward, and are restored when the URL is opened; leaving the timeline SHALL NOT carry them elsewhere. Selections that refer to a project or tag that no longer exists SHALL be ignored.

#### Scenario: Filter by one project
- **WHEN** the day has entries for tasks in Atlas, Billing and no project, and the user selects Atlas
- **THEN** the Atlas entries look as before, the other entries are dimmed in place, and a chip "project Atlas" with Clear is shown

#### Scenario: OR within a section
- **WHEN** the user selects projects Atlas and Billing in week view
- **THEN** entries of Atlas tasks and entries of Billing tasks are undimmed and all others are dimmed

#### Scenario: AND across sections
- **WHEN** the user selects project Atlas and tag urgent
- **THEN** only entries whose task is in Atlas and tagged urgent are undimmed

#### Scenario: No project
- **WHEN** the user selects "No project"
- **THEN** entries whose task has no project, and entries of deleted tasks, are undimmed and all others are dimmed

#### Scenario: Lanes do not reshuffle
- **WHEN** three entries overlap and a filter dims the middle one
- **THEN** each entry keeps its third of the column width and its position

#### Scenario: Dimmed entries are inert
- **WHEN** an entry is dimmed and the user clicks it, drags it, or presses J/K to move focus
- **THEN** no sheet opens, the entry does not move, and focus skips it

#### Scenario: Create while filtered
- **WHEN** a filter is active and the user drags on empty space
- **THEN** a pending range is proposed and the task picker opens as without a filter

#### Scenario: Keyboard
- **WHEN** the user presses F, types "atl", presses Enter, then presses ⇧F
- **THEN** the popover opens with search focused, Atlas is selected and the popover closes, and then the filter is cleared

#### Scenario: Persists across views and Back
- **WHEN** Atlas is selected in day view and the user presses W, then →, then browser Back
- **THEN** week view, the next week, and the previous week are all filtered to Atlas

#### Scenario: Shared URL
- **WHEN** the user opens a timeline URL that carries the Atlas filter
- **THEN** the timeline opens filtered to Atlas with its chip shown

#### Scenario: Unknown selection ignored
- **WHEN** the URL refers to a project that has been deleted
- **THEN** that selection is ignored and no chip is shown for it

### Requirement: Select and Create modes
The desktop timeline SHALL offer two interaction modes in day and week views, with a toggle in the toolbar next to the range control: **Create** (the default, the existing behavior) and **Select**. `V` SHALL toggle between the modes. `C` SHALL keep opening the new-task palette on the timeline as on every other tab. In month view the toggle SHALL be disabled, labelled "day & week only", and `V` SHALL do nothing. Switching to Create SHALL clear the selection. In Select mode:
- a click on an entry SHALL toggle it, and SHALL NOT open the entry sheet;
- a shift-click SHALL select every entry between the last clicked entry and the clicked one, ordered by start time;
- a drag on empty space SHALL draw a marquee that selects every entry it touches (adding to the selection with Shift, removing from it with Alt), and SHALL NOT propose a new range or open the task picker;
- entries SHALL NOT be moved or resized by dragging;
- `J`/`K` SHALL move focus, `Space` SHALL toggle the focused entry, and `⌘A`/`Ctrl+A` SHALL select every visible entry;
- `Esc` SHALL clear the selection, or return to Create when nothing is selected.

A selected entry SHALL show a ring in the text color (not the accent color), so the ring stays visible on every task color, plus a check badge. While at least one entry is selected, a floating selection bar SHALL show the count and the summed duration ("5 entries · 3h 20m selected"), a Delete action and an Esc hint. The status bar SHALL show the current mode, and the selection count and selection keys while in Select mode. The selection SHALL contain only entries that exist and are visible in the current range; entries that a timeline filter dims SHALL NOT be selectable.

#### Scenario: Toggle into Select and pick entries
- **WHEN** the user is in day view in Create mode, presses `V`, and clicks two entries
- **THEN** the mode shows Select, both entries show the selected ring, no entry sheet opens, and the selection bar reads "2 entries · <summed duration> selected"

#### Scenario: Shift-click extends by start time
- **WHEN** in Select mode the user clicks the 09:15 entry and then shift-clicks the 10:40 entry, with a 09:45 entry between them
- **THEN** all three entries are selected

#### Scenario: Marquee selects instead of creating
- **WHEN** in Select mode the user drags on empty space across two entries in week view
- **THEN** both entries are selected, no pending range is shown, and no task picker opens

#### Scenario: Esc clears, then leaves Select
- **WHEN** three entries are selected and the user presses `Esc` twice
- **THEN** the first press clears the selection and hides the bar, and the second press returns the timeline to Create mode

#### Scenario: C still creates a task
- **WHEN** the timeline is in Select mode and the user presses `C`
- **THEN** the new-task palette opens and the mode is unchanged

#### Scenario: Month view disables the toggle
- **WHEN** the user switches to month view
- **THEN** the mode toggle is disabled with the label "day & week only", and pressing `V` changes nothing

#### Scenario: Create mode unchanged
- **WHEN** the timeline is in Create mode and the user drags on empty space or clicks an entry
- **THEN** a pending range with the task picker is proposed, or the entry sheet opens, exactly as before

### Requirement: Bulk delete of selected entries
Deleting a selection of entries, from the selection bar Delete action or with `⌫`/`Delete`, SHALL remove every selected entry as a single undoable operation without a confirmation dialog, clear the selection, and show one undo toast "Deleted N entries" (or "Deleted 1 entry"). One undo SHALL restore every deleted entry with its original id and fields, and redo SHALL delete them again. A running entry in the selection SHALL be deleted like any other entry. Selected entries in different year documents SHALL be deleted by the same operation.

#### Scenario: Delete five entries
- **WHEN** five entries are selected and the user presses `⌫`
- **THEN** the five entries disappear, the selection bar hides, and one toast "Deleted 5 entries" with Undo is shown

#### Scenario: One undo restores all
- **WHEN** five entries were deleted together and the user presses `Ctrl+Z` once
- **THEN** all five entries return with their original ids, tasks, times and notes

#### Scenario: Selection across a year boundary
- **WHEN** week view spans 31 December and 1 January, one selected entry is on each day, and the user deletes the selection
- **THEN** both entries are deleted, and one undo restores both

### Requirement: Phone entry selection
The phone timeline SHALL NOT show a mode toggle and SHALL NOT start a selection with a long-press, because a long-press already starts a drag. The entry sheet SHALL instead offer a "Select" action that closes the sheet, selects that entry, and enters selection. While selecting, a tap on an entry SHALL toggle it instead of opening its sheet, and a bottom bar SHALL show the count and summed duration with Delete and Done. Done, or deselecting the last entry, SHALL end the selection and return taps to opening the sheet. Delete SHALL follow the bulk delete rules.

#### Scenario: Start selecting from the sheet
- **WHEN** on phone the user taps an entry, then taps "Select" in its sheet
- **THEN** the sheet closes, that entry is selected, and the bottom bar shows "1 entry · <duration>" with Delete and Done

#### Scenario: Tap toggles while selecting
- **WHEN** one entry is selected on phone and the user taps two more entries
- **THEN** three entries are selected and no sheet opens

#### Scenario: Long-press still drags
- **WHEN** on phone the user long-presses an entry for 250 ms and drags it
- **THEN** the entry moves as before, and no selection starts

#### Scenario: Done exits
- **WHEN** entries are selected on phone and the user taps Done
- **THEN** the selection clears, the bar hides, and tapping an entry opens its sheet again
