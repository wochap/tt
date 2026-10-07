## 1. Scaffold

- [ ] 1.1 `web/` with Vite + React + TypeScript + Tailwind + shadcn/ui init, pnpm, vitest, testing-library, Playwright; `packages/domain` workspace package
- [ ] 1.2 Tailwind theme from the design's Mocha and Latte token blocks (frame 13), `data-theme` switching, Phosphor icons, Inter; fix Latte `--c-faint` to a distinct AA value
- [ ] 1.3 CI script: lint, typecheck, vitest, build; `tt-server --web-dir web/dist` documented

## 2. Shared domain (TypeScript)

- [ ] 2.1 Document schema types matching `tt-core` (index, workspace, entries-YYYY); typed selectors and mutators with seq allocation and validation
- [ ] 2.2 Interval union, summed/wall/overlap totals, lane layout (`useLaneLayout` core as pure function)
- [ ] 2.3 Round + flatten with modes and groupings; quick-create parser; time/range parsing
- [ ] 2.4 Shared fixtures under `fixtures/` consumed by vitest and by Rust tests in `tt-core` (add the Rust side consumer)

## 3. Offline sync

- [ ] 3.1 SharedWorker repo with IndexedDB storage and websocket adapter; dedicated-worker fallback; MessageChannel adapter per tab
- [ ] 3.2 Auth store (token, index id), ticket fetch before each connect, reconnect backoff
- [ ] 3.3 `SyncStatus` from peer state; PWA via vite-plugin-pwa; logout wipe with unsynced count

## 4. App shell

- [ ] 4.1 Login screen (frame 6), routing, theme switch, settings (frame 8)
- [ ] 4.2 Command palette (frame 7) with all six actions; shortcut sheet (frame 22) with scoped groups; global key handling
- [ ] 4.3 Empty states (frame 14) and states sheet conformance (frame 23) for buttons, rows, chips

## 5. Timeline

- [ ] 5.1 `TimelineGrid`, `NowLine`, `EntryBlock` with lanes; day view (frame 1), week view with side panel (frame 2), month view (frame 3); jump-to-date popover (frame 16); snap menu (frame 17)
- [ ] 5.2 `RunningStrip` with ticking elapsed, stop, stop all
- [ ] 5.3 dnd-kit: create on drag with `TaskPicker` on release (frame 15), move, resize handles, re-link drop target, snapping, Alt/Shift modifiers, threshold
- [ ] 5.4 Entry sheet (frame 4): fields, chips, split, move to task, duplicate, delete, validation
- [ ] 5.5 Undo/redo stack with toasts; `TotalsBar` (summed, wall, overlap); phone day view with collapsible totals (frame 11)

## 6. Tasks

- [ ] 6.1 Tasks table (frame 5) with filters, fuzzy search with metadata highlight (frame 19), `QuickCreateInput` with Ctrl+Enter create-and-start; phone tasks (frame 12)
- [ ] 6.2 Board view (frame 21) with dnd between state columns
- [ ] 6.3 Task detail (frames 9, 18): metadata editor, markdown Edit/Preview, entries list with scope toggle, start/stop

## 7. Reports and export

- [ ] 7.1 Reports (frame 10) with range presets and custom range with times (frame 20), grouping bars
- [ ] 7.2 `RoundFlattenPreview` with options, side-by-side raw/flattened, badges; JSON and CSV download

## 8. Verification

- [ ] 8.1 Playwright: login, offline reload, two-tab propagation, drag create/move/resize/re-link, concurrent lanes, rename propagation, export preview equals domain output
- [ ] 8.2 Visual check against design frames in both flavors; contrast audit of small text
- [ ] 8.3 Build served by `tt-server --web-dir`, end-to-end with daemon-created data
