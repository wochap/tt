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
The sync status SHALL name the member the app currently syncs with (for example "Synced · laptop-b"), and Settings SHALL list the known members with which one is current.

#### Scenario: Failover visible
- **WHEN** the app switches from `laptop-a` to `laptop-b`
- **THEN** the status bar shows `laptop-b` without a reload

### Requirement: Single install across members
The app installed from any member SHALL work against every member of the same root. Signing out SHALL wipe local data and the endpoint list, as it does today.

#### Scenario: Second install not needed
- **WHEN** the user has installed the app from `laptop-a` and later only `laptop-b` is up
- **THEN** the installed app continues to sync through `laptop-b` and the user is never asked to install from `laptop-b`
