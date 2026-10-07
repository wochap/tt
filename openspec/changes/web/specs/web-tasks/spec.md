## ADDED Requirements

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
