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
- **Sync status** (`src/sync/pending.ts`): every sync message from the server
  carries its heads; local changes beyond them are "pending". The status bar
  shows Synced, Syncing · n changes, or Offline · n changes queued.
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
