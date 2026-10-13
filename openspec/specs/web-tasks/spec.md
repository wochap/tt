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

### Requirement: Sticky project for new tasks
While the device setting "New tasks use the last project" is on, the web app SHALL remember on this device the project of the last task created with a project, and SHALL offer it as the default project for the next task created from text. In quick create the default SHALL appear as a removable `@<project>` chip ahead of the caret. Typing an `@project` token SHALL replace the chip for that task. Pressing ⌫ at the start of the input SHALL remove the chip for that task only. The create rows of the task picker and the command palette SHALL name the default project they will apply. A task created with a project SHALL make that project the new default. A task created without a project, including one whose chip was removed, SHALL keep the previous default. A default whose project no longer exists SHALL be ignored, and no chip SHALL be shown. While the setting is off, no default SHALL be applied or shown, but the last project SHALL still be remembered.

#### Scenario: Default follows the last created task
- **WHEN** the setting is on, the user creates "Write docs @foo", then opens quick create again
- **THEN** quick create starts with an `@foo` chip, and creating "Fix typo" gives that task project foo

#### Scenario: Explicit project replaces the default
- **WHEN** the default is foo and the user creates "Plan sprint @algo"
- **THEN** the task gets project algo, and from then on quick create starts with an `@algo` chip

#### Scenario: Removing the chip affects one task
- **WHEN** the default is foo and the user presses ⌫ at the start of quick create, then creates "Personal errand"
- **THEN** that task has no project, and the next quick create still starts with `@foo`

#### Scenario: Picker create row applies the default
- **WHEN** the default is foo and the user types "Standup" in the timeline task picker and chooses the create row
- **THEN** the create row names project foo, and the new task has project foo

#### Scenario: Deleted project is ignored
- **WHEN** the default is foo and project foo no longer exists
- **THEN** quick create shows no chip, and a new task without `@project` has no project

#### Scenario: Setting off
- **WHEN** the setting is off and the last created task had project foo
- **THEN** quick create shows no chip and new tasks get a project only from an explicit `@project`

### Requirement: Task multi-select
The task list SHALL have a checkbox column that always reserves its width, so that showing a checkbox never shifts the row. A row's checkbox SHALL be visible on hover, on keyboard focus, and on every row once any task is selected. Clicking a checkbox SHALL toggle that task. Shift-click SHALL select the range of visible rows between the last toggled row and the clicked row. `Space` SHALL toggle the focused row, and `⇧J`/`⇧K` SHALL extend the selection while moving focus. The header checkbox and `⌘A`/`Ctrl+A` SHALL select every task in the current filter and search. On the board, `⌘`/`Ctrl`-click or `Space` on the focused card SHALL toggle a card, and a selected card SHALL show the selected ring and check badge. On phone, rows SHALL show checkboxes and an action bar while a selection exists. While at least one task is selected, a selection bar SHALL show "N tasks · M entries" (M = the total time entries of the selected tasks), "Select all K in view" (`⌘A`), Delete and an Esc hint. `Esc` SHALL clear the selection. The selection SHALL contain only tasks that still exist; changing the filter or search SHALL drop tasks that no longer match.

#### Scenario: Range select by shift-click
- **WHEN** the user checks task #44 and shift-clicks the checkbox of #41, three rows below
- **THEN** #44, the two rows between them and #41 are selected, and the bar shows "4 tasks · <entries> entries"

#### Scenario: Select all in the current filter
- **WHEN** the Project filter is Atlas, 7 tasks match, and the user presses `⌘A`
- **THEN** exactly those 7 tasks are selected

#### Scenario: Checkbox column does not shift rows
- **WHEN** no task is selected and the user hovers a row so its checkbox appears
- **THEN** the row's title and the other columns stay at the same horizontal position

#### Scenario: Board card selection
- **WHEN** in board view the user `⌘`-clicks two cards
- **THEN** both cards show the selected state and the selection bar shows "2 tasks · <entries> entries"

#### Scenario: Esc clears
- **WHEN** three tasks are selected and the user presses `Esc`
- **THEN** no task is selected and the selection bar is hidden

### Requirement: Bulk delete tasks
Deleting a task selection, from the selection bar Delete action or with `⌫`, SHALL follow the rules of single task delete applied to the whole set. If no selected task has time entries in any year, every selected task SHALL be deleted immediately. If any selected task has entries, nothing SHALL be deleted until the user confirms one dialog titled "Delete N tasks and M entries?", which lists each selected task with entries and its entry count, has Cancel focused on open, and offers "Delete N tasks and M entries"; Cancel or `Esc` SHALL leave everything unchanged. The whole deletion, including all entries, SHALL be a single undoable operation with one undo toast ("Deleted N tasks" or "Deleted N tasks and M entries"). One undo SHALL restore every task with its id and short id and every entry with its id and fields. While the device is still loading entry documents, bulk delete SHALL refuse with the same "Still loading your entries…" error and delete nothing. Short ids of deleted tasks SHALL NOT be reused.

#### Scenario: Unused tasks go at once
- **WHEN** three selected tasks have no entries and the user presses `⌫`
- **THEN** the three tasks disappear without a dialog and one toast "Deleted 3 tasks" with Undo is shown

#### Scenario: Tasks with entries ask once
- **WHEN** #44 (17 entries), #37 (18 entries) and #41 (6 entries) are selected and the user chooses Delete
- **THEN** one dialog "Delete 3 tasks and 41 entries?" lists the three tasks with their counts, and nothing is deleted yet

#### Scenario: Confirm and undo
- **WHEN** the user confirms "Delete 3 tasks and 41 entries" and then presses `Ctrl+Z` once
- **THEN** the three tasks and 41 entries are first removed with one toast "Deleted 3 tasks and 41 entries", and are then all restored with their original ids and short ids

#### Scenario: Still loading
- **WHEN** a year document listed in the index has not loaded yet and the user deletes a selection
- **THEN** an error toast "Still loading your entries…" is shown and no task or entry is deleted
