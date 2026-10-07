# web-app-shell Specification

## Purpose
TBD - created by syncing change web. Login, theming (Mocha/Latte/system), routing, command palette, settings, shortcut sheet, PWA install.

## Requirements

### Requirement: Login and session
The app SHALL present a login form (username, password, "stay signed in", server endpoint override), call `POST /api/login`, store the token and index document id, and stay usable offline afterward until logout.

#### Scenario: Offline after login
- **WHEN** the user has logged in once and later loads the app without network
- **THEN** the app renders the timeline from local data and shows sync state "Offline"

### Requirement: Theming
The app SHALL offer Mocha, Latte and System themes implementing the design's token sets, with every small-text color at or above 4.5:1 contrast in both flavors.

#### Scenario: System theme
- **WHEN** theme is System and the OS switches to light
- **THEN** the app switches to Latte without reload

### Requirement: Command palette and shortcuts
`Ctrl+K` SHALL open a palette with: start tracking (fuzzy over tasks), stop a running entry, jump to date, switch view, create task, open settings. A `?` sheet SHALL list shortcuts grouped by scope (Global, Timeline, Lists, Entry sheet, Task detail) as in the design.

#### Scenario: Start from palette
- **WHEN** the user types a ticket id in the palette and presses Enter on the match
- **THEN** a running entry is created for that task and the running strip updates

### Requirement: Settings
Settings SHALL include server endpoint (with reachability indicator), week start, snap grid (5/10/15), time zone, theme, visible hours, and logout that wipes local data and shows the count of unsynced changes before confirming.

#### Scenario: Logout with unsynced changes
- **WHEN** the user clicks logout while 12 changes are unsynced
- **THEN** a dialog states 12 unsynced changes will be lost and requires explicit confirmation

### Requirement: Empty states
Day, week, month, tasks and reports SHALL render the design's empty states with a primary action for a user with no data.

#### Scenario: New user
- **WHEN** a user with no tasks opens the timeline
- **THEN** the empty state offers "Create your first task" and "Start tracking"
