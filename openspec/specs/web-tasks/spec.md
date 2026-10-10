# web-tasks Specification

## Purpose
TBD - created by syncing change web. List, board, detail, quick-create, fuzzy search, markdown editor, start/stop from task.

## Requirements

### Requirement: Task list and board
Tasks SHALL be shown as a table (seq, title, tags, metadata, today, total, start button) or a board by state with drag between columns, filterable by state, tags and project, with fuzzy search over title, tag names and metadata values that highlights matched metadata.

#### Scenario: Board move
- **WHEN** a card is dragged from Open to Done
- **THEN** the task state becomes done and the card stays in the Done column

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
