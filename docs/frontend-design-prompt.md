# Design brief: tt web GUI

Paste everything below this line into Claude Design.

---

Design the web frontend for **tt**, a task and time tracker built for power users. The product also has a scriptable CLI and a local daemon; the web GUI is the visual counterpart, used mostly for reviewing and correcting time entries and for browsing tasks. It must feel fast, keyboard-friendly and dense, closer to Linear or a calendar app than to a marketing-style SaaS.

## Stack and constraints

- React, TypeScript, Tailwind, shadcn/ui components, dnd-kit for drag and drop.
- Data layer is Automerge (CRDT). The app is offline-first: it loads and works with no network, and syncs when a connection exists. Design for optimistic, instant updates. Never show spinners for local writes.
- Installable PWA. Must work at desktop widths first; tablet and phone widths should degrade gracefully (single-column day view).
- Light and dark themes via shadcn tokens.
- No marketing pages. Only the app plus a login screen.

## Domain model

- **User**: one account. Users never see each other's data.
- **Project**: optional grouping for tasks. Has name and color.
- **Tag**: free-form labels on tasks, user-scoped. Most users group with tags rather than projects.
- **Task**: `seq` (short stable number, shown as `#42`), `title`, optional markdown `description`, `tags[]`, optional `project`, `metadata` as key/value pairs (for example `ticket: PROJ-123`, `url: https://jira/...`), `state: open | done | archived`.
- **Time entry**: `task`, `start`, `end` (null while running), optional `note`. **Several entries can be running at once.** Entries can overlap. Renaming a task updates every entry's displayed title because entries only reference the task id.

## Screens

### 1. Login

Username and password. Remembers session. Once logged in the app works offline until the user logs out.

### 2. Timeline (primary screen)

Calendar-style views with a **day / week / month** switcher and date navigation (previous, next, today, jump to date). Keyboard shortcuts for all navigation.

- **Day view**: vertical time axis, one column. Overlapping or concurrent entries render side by side, never stacked or hidden. Running entries extend to "now" with a live-updating edge.
- **Week view**: seven columns, same rendering rules. Week starts on Monday by default; configurable.
- **Month view**: grid of days, each day shows total tracked time and the top tasks. Clicking a day opens the day view.
- A persistent **"running now"** strip at the top listing every running entry with elapsed time and a stop button each, plus "stop all".
- A **totals** footer or side panel for the visible range showing both the summed duration and the wall-clock duration (they differ when entries overlap).

Interactions on the timeline (dnd-kit):
- Drag on empty space to create an entry; a task picker appears on release.
- Drag an entry body to move it in time. Drag its top or bottom edge to resize.
- Drag an entry onto a task in the side panel to re-link it to that task.
- Click an entry to open an edit sheet: task picker, start, end, note, delete, split at a time, "move to task".
- Snap to a configurable grid (5, 10, 15 min). Hold a modifier to disable snapping.
- Undo and redo for all edits.

### 3. Tasks

List or board of tasks with fuzzy search over title, tags and metadata values (searching a ticket id must be instant). Filters by tag, project and state. Quick-create: a single input where typing `Fix login bug +backend +urgent` creates a task with two tags.

Task detail: editable title, tags, project, metadata key/value editor, markdown description editor with preview, state toggle, and a list of this task's entries with total time. "Start tracking" button, which creates a running entry.

### 4. Command palette

`Ctrl+K` opens a palette for: start tracking a task (fuzzy), stop a running entry, jump to date, switch view, create task, open settings.

### 5. Reports and export

Pick a range (day, week, month, custom range with optional times). Show totals grouped by task, by tag and by project. An **export panel** with:
- raw JSON or CSV of entries;
- a "rounded and flattened" export: entries are rounded up to a grid (15 min default), then pushed so they no longer overlap, then grouped either per entry or per task per day. Show a live preview of the result next to the raw data so the user can see how the rounding changed their day.

### 6. Settings

Server endpoint, week start day, default snap grid, time zone, theme, logout (which wipes local data).

## Design requirements

- Dense information display with generous hit targets for drag handles.
- Every destructive action is undoable rather than confirmed with a dialog.
- Concurrency must be visually obvious: two running entries should never be mistaken for one.
- Color comes from project color first, tag color second, neutral otherwise.
- Show sync state subtly (synced, syncing, offline) in the status bar, never as a blocking banner.
- Include empty states for a brand-new user.

## Deliverables

1. Component inventory mapped to shadcn/ui primitives.
2. High-fidelity screens for day, week and month views, the task list, the task detail, the entry edit sheet, the command palette, reports/export and settings, in both themes.
3. Interaction specs for every drag operation (create, move, resize, re-link) including snapping and overlap rendering.
4. Responsive variants for a phone width of the day view and task list.
