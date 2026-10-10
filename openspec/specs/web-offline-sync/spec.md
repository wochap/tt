# web-offline-sync Specification

## Purpose
TBD - created by syncing change web. SharedWorker repo, IndexedDB, websocket ticket flow, sync status, logout wipe.

## Requirements

### Requirement: Repository in a SharedWorker with IndexedDB
The app SHALL run one Automerge repo per browser profile inside a SharedWorker (dedicated worker fallback) using IndexedDB storage; tabs SHALL connect to it through a MessageChannel adapter so that only the worker talks to the server.

#### Scenario: Two tabs
- **WHEN** an entry is started in tab A
- **THEN** tab B shows it running within one second without a server round trip

### Requirement: Websocket ticket flow
The worker SHALL obtain a single-use ticket from `POST /api/ws-ticket` before each websocket connection and SHALL never place the long-lived token in a URL.

#### Scenario: Reconnect
- **WHEN** the socket drops
- **THEN** the worker fetches a new ticket and reconnects with exponential backoff capped at 30 seconds

### Requirement: Sync status
The status bar SHALL show Synced, Syncing with pending count, or Offline with queued count, derived from the repo's peer state, never as a blocking banner.

#### Scenario: Back online
- **WHEN** network returns with queued changes
- **THEN** status moves Offline → Syncing → Synced and the queued count reaches zero

### Requirement: PWA
The app SHALL be installable and SHALL load its shell from cache when offline.

#### Scenario: Cold offline load
- **WHEN** the installed app is opened with no network
- **THEN** the shell loads from cache and local documents render

### Requirement: Tickets come from the connected member
Before each websocket connection the worker SHALL obtain the single-use ticket from `POST /api/ws-ticket` of the member it is about to connect to, using the member-wide token, and SHALL never send a ticket from one member to another.

#### Scenario: Ticket on failover
- **WHEN** the worker fails over from `laptop-a` to `laptop-b`
- **THEN** it requests a new ticket from `laptop-b` and opens `laptop-b`'s `/sync` with it
