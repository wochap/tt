# task-management Specification

## Purpose
TBD - created by syncing change core-and-daemon. Tasks, short ids, fields, editor-based editing, projects, tags, and search.

## Requirements

### Requirement: Tasks carry a stable short id
Each task SHALL have a uuid and a per-user monotonic integer `seq` that is never reused, shown as `#<seq>` in all human output and accepted anywhere a task id is expected.

#### Scenario: Done task keeps its number
- **WHEN** task #12 is marked done and a new task is created
- **THEN** the new task receives #13 and #12 still resolves to the done task

#### Scenario: Two offline devices allocate the same seq
- **WHEN** both devices create a task with seq 20 offline and then sync
- **THEN** after sync exactly one task has seq 20, the later-created task receives the next free seq, and a `task.updated` event is emitted for it

### Requirement: Task fields
A task SHALL have `title` (required, non-empty), optional markdown `description`, a set of tags, an optional project, a `metadata` map of string keys to string values, a `state` of `open`, `done` or `archived`, and `created`/`updated` timestamps.

#### Scenario: Quick-create syntax
- **WHEN** the user runs `tt task add "Fix login PROJ-123" +backend +urgent @web ticket:PROJ-123`
- **THEN** a task is created with that title, tags `backend` and `urgent` (created if missing), project `web`, and metadata `ticket=PROJ-123`

#### Scenario: Rename propagates to entries
- **WHEN** a task's title is changed and `tt time ls` is run
- **THEN** every entry of that task shows the new title

### Requirement: Editor-based editing
`tt task edit <id>` SHALL open `$EDITOR` (falling back to `$VISUAL`, then `vi`) on a markdown file with YAML frontmatter (title, tags, project, state, metadata) and the description as body, and apply the result on save.

#### Scenario: Unparseable frontmatter
- **WHEN** the saved file has invalid frontmatter
- **THEN** no change is applied, the temp file is kept, and its path is printed with exit code 4

### Requirement: Projects and tags
Projects SHALL have a unique name, optional color, and archived flag. Tags SHALL be user-scoped names with optional color. Both SHALL be creatable, renameable and listable, and a task may have zero projects.

#### Scenario: Rename tag
- **WHEN** tag `backend` is renamed to `be`
- **THEN** every task previously tagged `backend` lists `be`

### Requirement: Search and listing
`tt task ls` SHALL filter by state (default open), tag and project, and `tt task find <text>` SHALL fuzzy-match against title, tag names and metadata values, returning matches ordered by score.

#### Scenario: Ticket id search
- **WHEN** a task has metadata `ticket=PROJ-123` and the user runs `tt task find proj123`
- **THEN** that task is the first result
