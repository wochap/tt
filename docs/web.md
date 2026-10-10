# tt web app

`web/` is a React + TypeScript + Vite app that is a full offline peer of
tt-server: it keeps every document in the browser, edits them locally and
syncs over `/sync`. The design source is `design/project/tt Web.dc.html`
(frames 1–23, Catppuccin Mocha and Latte).

## Build, run, test

```sh
pnpm install --frozen-lockfile
pnpm build                 # tsc + vite → web/dist (PWA, service worker, wasm)
pnpm run ci                # lint, typecheck, vitest (domain + web), build
pnpm e2e                   # Playwright against tt-server --web-dir web/dist

# development: vite on :5173, /api and /sync proxied to TT_SERVER
just server                # tt-server on 127.0.0.1:8080 (plain HTTP)
pnpm --filter web dev
```

`just web` and `just web-e2e` wrap the same commands; `just ci` runs them
after the Rust checks. The e2e suite builds the bundle and `tt-server`,
creates throwaway accounts in a temp database (`web/e2e/serve.sh`) and uses
Playwright's Chromium, or a system Chrome (`TT_E2E_CHROME`).

## Architecture

- **Repo worker** (`src/sync/worker.ts`): one Automerge repo per browser
  profile in a SharedWorker (a dedicated worker per tab where SharedWorker is
  missing, e.g. Safari on iOS), stored in IndexedDB (`tt`). Tabs are peers of
  the worker over a MessageChannel; only the worker talks to the server.
- **Ticket socket** (`src/sync/ticket-socket.ts`): before every connection the
  worker asks `POST /api/ws-ticket` for a single-use ticket and opens
  `/sync?ticket=…`; the bearer token never goes into a URL. Reconnects back
  off exponentially, capped at 30 s; a 401 stops and shows "Sign in again".
- **Member failover** (`src/sync/endpoints.ts`, `member-endpoints.ts`): the
  worker syncs with one member server of the root at a time. It keeps the
  members' client URLs (`{server_id, name, public_url, last_ok}`) with the
  session, seeded with the sign-in server and refreshed from `/api/peers` of
  the member it just connected to. Each attempt checks `/api/health` (5 s
  timeout; a `protocol` outside `SUPPORTED_PROTOCOLS` marks the member
  incompatible and skips it), takes a ticket from that member and opens its
  `/sync`. The last member that synced goes first; after a drop the next
  member is tried at once, and only a full pass without success backs off.
  A 401 from every member that answered means "Sign in again". All members
  sync into the same IndexedDB repo; signing out drops the list with the
  session. Settings, Sync, Servers lists the members, the current one marked.
- **Sync status** (`src/sync/pending.ts`): every sync message from the server
  carries its heads; local changes beyond them are "pending". The status bar
  names the member it syncs with: "Synced · laptop-a · 14s ago", "Syncing 3
  changes · laptop-a", or "laptop-a unreachable · 5 changes saved here" ("Offline
  · 5 changes saved here" when several members are known); its tooltip adds
  the short server id (first 8 characters), last sync and pending count.
- **Session and server identity** (`src/sync/auth.ts`): login stores the
  token, index document id and the server's `{id, name}` together, and
  `/api/me` refreshes them once per load, so the status bar, phone header and
  account menu ("wochap on laptop-a") name the server offline too. Before any
  session, the login page asks `/api/health` for the name ("Sign in to
  laptop-a", falling back to the host) and shows the URL. A 409
  `account_conflict` shows a banner (the name is also used on another server;
  the admin renames it with `tt-server user rename`) instead of the
  wrong-password error, and Sign in stays disabled until the username changes.
- **Store** (`src/data/store.ts`): index → workspace → `entries-YYYY`, merged
  into one view. Writes mirror tt-core: an entry lives in the document of its
  start year (moving it across years moves it between documents; the first
  entry of a new year creates the document and lists it in the index). Undo
  and redo are a session stack of record snapshots (not Automerge history).
- **Domain** (`packages/domain`): TypeScript port of the pure parts of
  tt-core (schema, operations, lanes, totals, round + flatten, quick-create,
  time parsing, search, export). `fixtures/*.json` are run by both test
  suites so the implementations cannot drift.
- **PWA**: vite-plugin-pwa precaches the shell (including the Automerge wasm);
  `/api` is network-first. Logout revokes the token, wipes IndexedDB and the
  runtime caches; the precached shell stays so the login page loads offline.

## Settings

Theme (System / Latte / Mocha), week start, default snap grid, time zone and
visible hours are per device (localStorage), not synced.
