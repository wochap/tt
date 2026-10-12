# web-tasks Specification

## Purpose
TBD - created by syncing change web. List, board, detail, quick-create, fuzzy search, markdown editor, start/stop from task.

## Requirements

### Requirement: Task list and board
Tasks SHALL be shown as a table (seq, title, tags, metadata, today, total, start button) or a board by state with drag between columns, filterable by state, tags and project, with fuzzy search over title, tag names and metadata values that highlights matched metadata. On touch devices a board card SHALL start dragging only after a press-and-hold of about 250 ms; a quicker vertical swipe SHALL scroll instead and a tap SHALL NOT drag.

#### Scenario: Board move
- **WHEN** a card is dragged from Open to Done
- **THEN** the task state becomes done and the card stays in the Done column

#### Scenario: Touch long-press moves a card
- **WHEN** on a touch device the user presses and holds a board card for about 250 ms and drags it onto the Done column
- **THEN** the task state becomes done, whereas a quick swipe starting on the same card scrolls the board and does not move it

### Requirement: Quick create
A single input SHALL parse `title +tag @project key:value`, creating tags and projects as needed; Ctrl+Enter SHALL create and start tracking.

#### Scenario: Parse
- **WHEN** the user types `Fix login +backend +urgent @web ticket:PROJ-123` and presses Ctrl+Enter
- **THEN** a task with that title, two tags, project web and metadata ticket is created and a running entry starts

### Requirement: Task detail
Task detail SHALL show editable title, project, tags, state, metadata key/value editor, markdown description with Edit/Preview toggle (Ctrl+E), entries list with This week / All time toggle and totals, and Start tracking or Stop depending on running state.

#### Scenario: Rename
- **WHEN** the title is edited
- **THEN** every entry on the timeline shows the new title without reload

### Requirement: Old short ids point to renumbered tasks
When the task search query or the command palette query is a short id `#<n>` and a task lists `<n>` in its `previous_seqs`, the results SHALL include, directly below the task that holds `#<n>` now (or alone, when no task holds it), a muted hint row reading "#<to> <title> was renumbered from #<n>" with an "Open #<to>" action. The hint row SHALL NOT be selectable by keyboard navigation and SHALL NOT start tracking.

#### Scenario: Renumbered id searched
- **WHEN** task #12 "Rotate staging certs" was renumbered to #17, another task now holds #12, and the user searches "#12"
- **THEN** the task holding #12 is listed, followed by the hint "#17 Rotate staging certs was renumbered from #12", and clicking "Open #17" opens task #17

#### Scenario: Palette skips the hint
- **WHEN** the command palette shows a renumber hint row and the user presses the down arrow from the row above it
- **THEN** the selection moves past the hint to the next selectable item

### Requirement: Delete task
The web app SHALL let the user delete a task from the task detail page and from a `⋯` overflow menu on the task list rows (desktop and phone) and on the board cards (also via the ⌫ shortcut on the focused row, card or open task; `.` opens the menu of the focused row or card). Delete SHALL be the last item of that menu, preceded by a separator, in destructive style. Opening the menu on a board card SHALL NOT start dragging the card. Deleting a task that has no time entries SHALL happen immediately and show an undo toast. Deleting a task that has time entries, in any year, SHALL NOT delete anything until the user confirms in a dialog that states the number of entries and offers "Delete task and N entries"; confirming deletes the task and all its entries. Cancelling SHALL leave the task and its entries unchanged. A running entry of the task counts as an entry. The deletion, including any entries, SHALL be a single undoable operation: undo restores the task with the same id and short id `#<seq>` and every deleted entry with the same id and fields, and redo deletes them again. Deleting the task shown on the detail page SHALL return the user to the task list. The short id of a deleted task SHALL NOT be reused.

#### Scenario: Delete an unused task
- **WHEN** task #31 has no entries and the user chooses Delete on its detail page
- **THEN** the task disappears from the list and board without a dialog, the app returns to the task list, and an undo toast is shown

#### Scenario: Task with entries asks first
- **WHEN** task #12 has 3 entries (one of them in a previous year) and the user chooses Delete
- **THEN** a dialog states that the task has 3 entries and offers "Delete task and 3 entries" and Cancel, and nothing is deleted yet

#### Scenario: Confirm force delete
- **WHEN** the user confirms "Delete task and 3 entries"
- **THEN** task #12 and its 3 entries are gone from the list, board and timeline, and one undo toast is shown

#### Scenario: Cancel keeps everything
- **WHEN** the dialog is open and the user presses Cancel or Escape
- **THEN** task #12 and its 3 entries are unchanged

#### Scenario: Undo restores task and entries
- **WHEN** task #12 and its 3 entries were deleted and the user presses Ctrl+Z
- **THEN** task #12 returns with its title, tags, project, metadata and state, and the 3 entries return on the timeline with their original ids and times

#### Scenario: Seq is not reused
- **WHEN** task #31 (the highest seq) is deleted and a new task is created
- **THEN** the new task receives #32

#### Scenario: Board menu does not drag
- **WHEN** the user presses the `⋯` button on a board card and opens the menu
- **THEN** the menu opens, the card does not move, and choosing Delete starts the same flow as on the detail page
