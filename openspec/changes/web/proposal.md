## Why

The CLI covers scripting; reviewing and correcting a week of time entries, and browsing tasks visually, needs a GUI. The design is finished (handoff bundle in `design/`, 23 frames in Catppuccin Mocha and Latte). The server from `server` already speaks the browser's protocol, so the web app can be a full offline peer.

## What Changes

- New `web/` package: React + TypeScript + Vite + Tailwind + shadcn/ui + dnd-kit + `@automerge/automerge-repo` (+ react hooks), vitest + testing-library + Playwright.
- Repo runs in a SharedWorker with IndexedDB storage and a websocket adapter to `/sync`; tabs connect through a MessageChannel adapter. PWA with service worker so the app loads offline.
- Screens from the design: login, timeline (day/week/month) with lane layout for overlapping entries, running strip, entry sheet, drag create/move/resize/re-link with snapping and undo, tasks list/board/detail with quick-create and fuzzy search, command palette, reports with rounded/flattened export preview, settings, empty states, shortcut sheet.
- Domain logic shared with Rust by re-implementing the small pure parts in TypeScript (document schema types, lane layout, union/summed totals, round+flatten) with the same fixtures as the Rust tests.
- Built bundle served by `tt-server --web-dir`.

## Capabilities

### New Capabilities
- `web-app-shell`: login, theming (Mocha/Latte/system), routing, command palette, settings, shortcut sheet, PWA install.
- `web-offline-sync`: SharedWorker repo, IndexedDB, websocket ticket flow, sync status, logout wipe.
- `web-timeline`: day/week/month views, lanes, running strip, entry sheet, drag interactions, undo/redo, totals.
- `web-tasks`: list, board, detail, quick-create, fuzzy search, markdown editor, start/stop from task.
- `web-reports-export`: range reports, grouping, JSON/CSV download, rounded + flattened preview with per-entry and per-task-day grouping.

### Modified Capabilities
(none)

## Impact

- New toolchain (node, pnpm) alongside cargo; CI runs both.
- `tt-server` gains a documented `--web-dir` build artifact path; no API changes.
- Document schema in `tt-core` becomes a contract shared with TypeScript; add a JSON fixture set under `fixtures/` consumed by both test suites.
