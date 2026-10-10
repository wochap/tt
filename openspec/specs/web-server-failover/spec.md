# web-server-failover Specification

## Purpose

Lets one installed web app sync with whichever member server of a shared root is reachable, so a phone needs a single install and a single login instead of one per server.

## Requirements

### Requirement: Member endpoint list
After sign-in, the web app SHALL keep a list of member client URLs learned from `GET /api/peers` of any reachable member, persist it with the session, and refresh it on every successful connection. The member the user signed in to SHALL always be in the list. Revoked members SHALL be removed from the list on the next refresh.

#### Scenario: List learned after login
- **WHEN** the user signs in to `laptop-a`, which is paired with `laptop-b`
- **THEN** the stored endpoint list contains the client URLs of `laptop-a` and `laptop-b`

#### Scenario: Revoked member dropped
- **WHEN** `laptop-b` is revoked and the app next connects to `laptop-a`
- **THEN** `laptop-b` is removed from the endpoint list and never contacted again

### Requirement: Member selection and failover
The worker SHALL connect to one member at a time, trying the last member that synced successfully first and then the others in order of most recent success, with a per-attempt timeout of 5 seconds. When the current connection drops, it SHALL try the next member before backing off. A member that answers with an incompatible protocol version SHALL be skipped. All members SHALL sync into the same local repository.

#### Scenario: Origin server down
- **WHEN** the app was installed from `laptop-a`, `laptop-a` is unreachable and `laptop-b` is reachable
- **THEN** the app loads its shell from cache, connects to `laptop-b` within 10 seconds, and syncs without a new login

#### Scenario: All members down
- **WHEN** no member is reachable
- **THEN** the status shows Offline with the queued count, and the worker retries all members with exponential backoff capped at 30 seconds

#### Scenario: Incompatible member
- **WHEN** `laptop-b` reports a protocol version the app does not support
- **THEN** the app skips `laptop-b` and connects to another member

### Requirement: Current member display
The sync status SHALL name the member the app currently syncs with (for example "Synced · laptop-b"), and Settings SHALL list the known members with which one is current. When the app switches to another member, the status line SHALL show "Switched to <member>" with an accent tint for 4 seconds and then return to the normal status text; the switch SHALL NOT raise a toast or block any interaction. The sync tooltip SHALL summarize the other known members: "Also reachable: <names>" for members that answered their last health check, and "Other members: <names> (unreachable)" for members that did not. Each member row in Settings SHALL show the member's name, short id (first 8 characters), URL and one state: Connected (the member in use, marked "in use"), Reachable, Unreachable, or "Skipped: incompatible version (<version>)", with a note that the list is read-only and the app picks a member by itself.

#### Scenario: Failover visible
- **WHEN** the app switches from `laptop-a` to `laptop-b`
- **THEN** the status bar shows `laptop-b` without a reload

#### Scenario: Switch notice fades
- **WHEN** the app switches from `laptop-a` to `laptop-b`
- **THEN** the status line reads "Switched to laptop-b" with an accent tint, no toast appears, and after 4 seconds it reads "Synced · laptop-b"

#### Scenario: Tooltip lists reachable members
- **WHEN** the app syncs with `laptop-b` and `laptop-a` answered its last health check
- **THEN** the sync tooltip contains "Also reachable: laptop-a"

#### Scenario: Tooltip lists unreachable members
- **WHEN** the app syncs with `laptop-b` and `laptop-a` did not answer its last health check
- **THEN** the sync tooltip contains "Other members: laptop-a (unreachable)"

#### Scenario: Member states in Settings
- **WHEN** Settings → Sync lists `laptop-b` in use, `laptop-a` answering, `cloud` not answering, and `old` reporting an unsupported protocol with version 1.2.0
- **THEN** the rows read Connected with an "in use" marker, Reachable, Unreachable, and "Skipped: incompatible version (1.2.0)", each with its short id and URL

### Requirement: Single install across members
The app installed from any member SHALL work against every member of the same root. Signing out SHALL wipe local data and the endpoint list, as it does today.

#### Scenario: Second install not needed
- **WHEN** the user has installed the app from `laptop-a` and later only `laptop-b` is up
- **THEN** the installed app continues to sync through `laptop-b` and the user is never asked to install from `laptop-b`

### Requirement: Reachability of members not in use
While the app is connected to one member, it SHALL check the health endpoint of every other known member at most once per minute and record whether it answered and, when it answered, its reported version and protocol. The check SHALL NOT open a sync connection, SHALL NOT request a ticket, and SHALL stop while the app has no network or is signed out.

#### Scenario: Other member comes back
- **WHEN** `laptop-a` was unreachable and starts answering while the app syncs with `laptop-b`
- **THEN** within about a minute Settings shows `laptop-a` as Reachable and the app stays connected to `laptop-b`
