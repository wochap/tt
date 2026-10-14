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
`Ctrl+K` SHALL open a palette with: start tracking (fuzzy over tasks), stop a running entry, jump to date, switch view, create task, open settings. A `?` sheet SHALL list shortcuts grouped by scope (Global, Timeline, Lists, Entry sheet, Task detail) as in the design. The sheet SHALL list the selection keys: in Timeline, `V` (Select / Create), `Space` (toggle), `⌘A` (select all visible), `⌫` (delete selection) and `Esc` (clear selection); in Lists, `Space` (toggle), `⇧J`/`⇧K` (extend), `⌘A` (select all in filter) and `⌫` (delete selection). `C` SHALL remain listed under Global as new task.

#### Scenario: Start from palette
- **WHEN** the user types a ticket id in the palette and presses Enter on the match
- **THEN** a running entry is created for that task and the running strip updates

#### Scenario: Selection keys listed
- **WHEN** the user opens the `?` sheet
- **THEN** the Timeline group lists `V` Select / Create and the selection keys, the Lists group lists the multi-select keys, and `C` new task is still in Global

### Requirement: Settings
Settings SHALL include server endpoint (with reachability indicator), week start, a Start grid (off/5/10/15/30/60 min) and a Duration grid (5/10/15/30/60 min), time zone, theme, visible hours, a "New tasks use the last project" switch, and logout that wipes local data and shows the count of unsynced changes before confirming. The Start grid and Duration grid SHALL be stored on the device only, SHALL default to 15 min each, and SHALL carry helper text saying that the Timeline and Reports start from them and that changing them there overrides that view only. Start grid options that do not divide the chosen Duration grid SHALL be disabled with the hint "must divide duration", and choosing a Duration grid that the current Start grid does not divide SHALL set the Start grid to the largest valid value. A device that stored the previous single snap value SHALL get it as its Start grid (off stays off) and as its Duration grid (off becomes 15 min). The "New tasks use the last project" switch SHALL be on by default, SHALL be stored on the device only (not synced), and SHALL carry one line of helper text explaining that quick create starts with that project as a chip that ⌫ drops for one task.

#### Scenario: Logout with unsynced changes
- **WHEN** the user clicks logout while 12 changes are unsynced
- **THEN** a dialog states 12 unsynced changes will be lost and requires explicit confirmation

#### Scenario: Sticky project switch defaults on
- **WHEN** a user opens Settings on a device where the switch was never changed
- **THEN** "New tasks use the last project" is shown switched on, with its helper text

#### Scenario: Sticky project switch is per device
- **WHEN** the user turns the switch off in one browser
- **THEN** it stays off after reload in that browser, and another device keeps its own value

#### Scenario: Grid defaults
- **WHEN** a user opens Settings on a new device
- **THEN** Start grid and Duration grid both read 15 min and Start grid options 10, 30 and 60 min are disabled

#### Scenario: Duration change lowers the start
- **WHEN** Start grid is 15 min and the user selects Duration grid 10 min
- **THEN** Start grid becomes 10 min

#### Scenario: Old snap value migrated
- **WHEN** the device stored snap 10 min before this change
- **THEN** Settings shows Start grid 10 min and Duration grid 10 min

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

### Requirement: Remembered views
The Timeline tab SHALL reopen on the range (day, week or month) and date last shown on this device, and the Tasks tab SHALL reopen in the mode (list or board) last shown on this device. The remembered choices SHALL be stored on the device only and SHALL apply when the tab is opened without an explicit range, date or mode: from the top bar, its number shortcut, or by loading `/timeline` or `/tasks`. A remembered date SHALL apply only on the same calendar day it was saved, in the app's time zone; on any later day the timeline SHALL open on today in the remembered range. An explicit link or palette command that names a range, date or mode SHALL win over the remembered value and SHALL become the new remembered value. Phone layouts SHALL keep their fixed day view and list mode, but SHALL NOT overwrite the remembered desktop range or mode.

#### Scenario: Week view comes back
- **WHEN** the user switches the timeline to week, opens Tasks, then clicks the Timeline tab
- **THEN** the timeline opens in week view on the same week

#### Scenario: Date comes back on the same day
- **WHEN** the user jumps the day view to 3 Oct, reloads the app later that day and opens the Timeline tab
- **THEN** day view opens on 3 Oct

#### Scenario: New day falls back to today
- **WHEN** the user left the timeline in week view on 3 Oct and opens the app the next calendar day
- **THEN** the timeline opens in week view on the current week

#### Scenario: Board comes back
- **WHEN** the user switches Tasks to board, opens Reports, then presses `2`
- **THEN** Tasks opens as a board

#### Scenario: Explicit command wins
- **WHEN** the remembered mode is board and the user runs the palette command "Tasks · list"
- **THEN** Tasks opens as a list and list becomes the remembered mode

### Requirement: Segmented control selection
In every segmented control (for example Day/Week/Month, List/Board and the choices in Settings), the selected option's accent ring SHALL be fully visible, including at the rounded outer corners of the first and last options.

#### Scenario: First option selected
- **WHEN** week start is Monday, the first of three options
- **THEN** Monday's accent ring follows the control's rounded left corners with no part cut off

#### Scenario: Last option selected
- **WHEN** Month is selected in the Day/Week/Month control
- **THEN** Month's accent ring follows the control's rounded right corners with no part cut off

### Requirement: Undo toast countdown
Undo toasts SHALL appear at the bottom left for 8 seconds and SHALL show a 2 px bar that drains linearly over the remaining time. The countdown and the bar SHALL pause while the pointer is over the toast or focus is inside it, and resume afterwards. The toast's Undo action SHALL undo the operation it announces. `⌘Z`/`Ctrl+Z` SHALL keep working through the regular undo stack after the toast is gone.

#### Scenario: Bar drains and toast leaves
- **WHEN** an undo toast is shown and the user does not interact with it
- **THEN** its bar drains from full to empty and the toast closes after about 8 seconds

#### Scenario: Hover pauses
- **WHEN** the pointer rests on an undo toast for 5 seconds
- **THEN** the bar stops draining and the toast stays open until the pointer leaves

#### Scenario: Undo after the toast is gone
- **WHEN** the toast for "Deleted 5 entries" has closed and the user presses `Ctrl+Z`
- **THEN** the five entries are restored

### Requirement: App icon
The app's icon SHALL be the design's "Timeline blocks" mark: a rounded tile in the theme's base color carrying staggered, rounded entry blocks in mauve, peach, green and blue. The browser favicon, the web app manifest icons and the Apple touch icon SHALL all use this mark; the former "tt" lettermark SHALL NOT appear in any of them. At favicon sizes (16–32 px) the icon SHALL use the design's small variant: three thicker blocks (mauve, peach, green) and no tile outline. The favicon SHALL use Mocha colors when the operating system prefers a dark color scheme and Latte colors when it prefers a light one. The manifest SHALL list a scalable icon, 192 px and 512 px icons, and a 512 px maskable icon whose blocks stay inside the maskable safe zone.

#### Scenario: Favicon in a dark browser
- **WHEN** the app is open in a browser tab and the OS prefers a dark color scheme
- **THEN** the tab shows the three-block mark on a `#1e1e2e` tile in mauve, peach and green

#### Scenario: Favicon in a light browser
- **WHEN** the app is open in a browser tab and the OS prefers a light color scheme
- **THEN** the tab shows the three-block mark on a `#eff1f5` tile in `#8839ef`, `#fe640b` and `#40a02b`

#### Scenario: Installed app icon
- **WHEN** the user installs the PWA on a desktop or phone
- **THEN** the launcher and home screen show the four-block Timeline blocks mark, not the "tt" letters

#### Scenario: Maskable icon on Android
- **WHEN** an Android launcher crops the maskable icon to a circle or squircle
- **THEN** no block is cut off
