# web-app-shell Specification

## Purpose
TBD - created by syncing change web. Login, theming (Mocha/Latte/system), routing, command palette, settings, shortcut sheet, PWA install.

## Requirements

### Requirement: Login and session
The app SHALL present a login form (username, password, "stay signed in", server endpoint override) titled with the server's name and showing its URL, call `POST /api/login`, store the token, index document id and server identity, and stay usable offline afterward until logout. Below the Sign in button the form SHALL show the muted line "Works on all paired servers". A 409 `account_conflict` response SHALL show a dedicated error explaining that the account name is also used on another server and must be renamed by the admin, distinct from the wrong-password error.

#### Scenario: Offline after login
- **WHEN** the user has logged in once and later loads the app without network
- **THEN** the app renders the timeline from local data and shows sync state "Offline"

#### Scenario: Server name on login
- **WHEN** the login page loads from server `laptop-a`
- **THEN** it reads "Sign in to laptop-a" and shows the server URL

#### Scenario: One sign-in for all members
- **WHEN** the login page loads
- **THEN** the line "Works on all paired servers" appears under the Sign in button

#### Scenario: Account conflict
- **WHEN** login returns 409 `account_conflict`
- **THEN** the conflict error is shown, not the wrong-password error, and the user stays on the login page

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

### Requirement: Server section in settings
Settings SHALL include a Server section between Sync and Shortcuts showing this server's name, short id (copyable) and version, and a read-only list of peers from `GET /api/peers` with a status dot, name, state, last seen and address per row. The list SHALL render as a table on desktop and a stacked list on phone, SHALL refresh while the section is visible, and SHALL show an empty state ("Not paired with other servers") with the CLI commands `tt-server peer invite` and `tt-server peer join <code>`.

#### Scenario: Peers listed
- **WHEN** the user opens Settings on a server linked with `laptop-b`
- **THEN** the Server section shows `laptop-b` online with its address and last seen time

#### Scenario: Offline client
- **WHEN** Settings is open and the server is unreachable
- **THEN** the Server section shows the last loaded peer list marked as stale, not an error page

#### Scenario: Unpaired server
- **WHEN** `/api/peers` returns an empty list
- **THEN** the empty state with both CLI commands is shown

### Requirement: Empty states
Day, week, month, tasks and reports SHALL render the design's empty states with a primary action for a user with no data. The empty month state SHALL be a card placed in the page flow above the month grid, so it never covers day numbers or cells.

#### Scenario: New user
- **WHEN** a user with no tasks opens the timeline
- **THEN** the empty state offers "Create your first task" and "Start tracking"

#### Scenario: Empty month does not cover the grid
- **WHEN** month view shows a month without entries
- **THEN** the "No time tracked this month" card appears above the grid and every day number stays visible and clickable

### Requirement: Server identity in the shell
The status bar sync label and its tooltip, the phone header, and the account menu SHALL name the server the app syncs with (for example "Synced · laptop-a", "wochap on laptop-a"), using the identity stored at login so it is shown offline too.

#### Scenario: Offline label
- **WHEN** the app is offline after logging in to `laptop-a`
- **THEN** the tooltip says changes sync when laptop-a is reachable

### Requirement: Renumbered task notice
When the app's local workspace renumbers a task or receives a renumbering through sync, it SHALL show a toast "Task #<from> is now #<to> · <title>" with a View action, collapsing several renumberings into one "<n> tasks renumbered" toast. Task detail SHALL show "previously #<n>" for each entry in the task's `previous_seqs`.

#### Scenario: Single renumber
- **WHEN** sync renumbers "Fix login" from #20 to #31
- **THEN** a toast reads "Task #20 is now #31 · Fix login" and its View action opens task #31

#### Scenario: Several renumbers
- **WHEN** one sync renumbers three tasks
- **THEN** a single toast reads "3 tasks renumbered" and lists each change when expanded

### Requirement: Server not set up page
When `/api/health` reports `setup: "needs-decision"`, the app SHALL show a full-page state titled "<server name> is not set up" with the commands `tt-server init` and `tt-server peer join <code>` as copyable blocks, and SHALL NOT show the login form.

#### Scenario: Fresh server
- **WHEN** a browser opens the app on a server in `NeedsDecision`
- **THEN** the not-set-up page is shown instead of the login form

#### Scenario: Server becomes ready
- **WHEN** the server is initialized while the page is open
- **THEN** the app switches to the login form after its next health check
