## Context

Design bundle: `design/project/tt Web.dc.html` (frames 1–23, Mocha + Latte, token sheet in frame 13, dnd-kit drag spec cards, component inventory tables mapping to shadcn and listing custom components `TimelineGrid`, `EntryBlock`, `useLaneLayout`, `RunningStrip`, `NowLine`, `TaskPicker`, `RoundFlattenPreview`, `TotalsBar`, `QuickCreateInput`, `SyncStatus`). The design system under `design/project/_ds/` is reference only; the implementation uses Tailwind tokens derived from the design's two flavor blocks. Known leftover: Latte `--c-faint` equals `--c-muted`; implementation picks a distinct value that passes 4.5:1.

## Goals / Non-Goals

**Goals:**
- Pixel-faithful to the design in both flavors, dense, keyboard-first.
- Full offline operation after first login; local writes are instant.
- Same data semantics as the CLI (concurrent entries, task-id references, seq ids).

**Non-Goals:**
- Mobile native apps. Phone width is a responsive variant only.
- Server changes. Any API gap is filed back into the `server` change.
- Collaboration between users.

## Decisions

### D1. Automerge in a SharedWorker
`worker.ts` owns `Repo({ storage: IndexedDBStorageAdapter, network: [BrowserWebSocketClientAdapter(url with ticket)] })`. Each tab creates a `MessageChannelNetworkAdapter` to the worker, so tabs are peers of the worker and the worker is the only peer of the server. Fallback to a dedicated worker when `SharedWorker` is unavailable (Safari iOS). The worker handles ticket refresh: on connect it calls `/api/ws-ticket` with the stored token, then opens the socket.

### D2. Document access
`useDocument(indexId)` resolves the index, then workspace and the entries docs for the visible range (`entries-YYYY`), via `@automerge/automerge-repo-react-hooks`. A thin `domain/` layer provides typed selectors (tasks by seq, entries in range, running entries) and mutators that mirror `tt-core` semantics, including `seq` allocation and start<end validation. Entries are written to the year doc of their start date; moving an entry across years moves it between docs.

### D3. State and undo
Undo/redo is a client-side stack of inverse mutations (not Automerge history), scoped to the session, covering every timeline and task edit. Destructive actions never confirm; they toast with Undo.

### D4. Timeline rendering
`useLaneLayout(entries)` assigns lanes per overlap cluster (N equal lanes, 3 px gutter, per design spec). Running entries extend to `now` with a 1 s ticking edge. Drag spec from the design: 4 px threshold, snap to grid from settings, Alt frees to 1 min, Shift locks column in week view, 8 px edge handles for resize, drop on task panel re-links. Create on drag release opens the anchored `TaskPicker`.

### D5. Shared pure logic
`packages/domain` (TypeScript) implements lane layout, interval union, summed/wall totals, round+flatten (modes up|nearest, grouping entry|task-day), quick-create parser (`title +tag @project key:value`), and time parsing used by inputs. Fixtures in `fixtures/*.json` are shared with the Rust tests so both implementations agree.

### D6. Theming
Tailwind config exposes CSS variables for both flavors from the design's token blocks; `data-theme="mocha|latte"` on `<html>`, `system` follows `prefers-color-scheme`. shadcn components are themed through the same variables. Phosphor icons.

### D7. PWA
`vite-plugin-pwa` with precached app shell and network-first for `/api`. Install prompt in settings. Logout clears IndexedDB, caches and token.

### D8. Testing
vitest for domain and components, Playwright for: login, offline reload, drag create/move/resize, concurrent entries lanes, rename propagation, export preview numbers equal to domain function output.

## Risks / Trade-offs

- [SharedWorker support gaps] → dedicated worker fallback, documented.
- [Large entries docs in the browser] → only load years in view; month view uses aggregated per-day totals computed once per doc change.
- [Two implementations of rounding drift] → shared fixtures in both test suites, CI fails on divergence.
- [Websocket ticket expiry during reconnect storms] → worker fetches a fresh ticket on every reconnect with backoff.

## Open Questions

- None.
