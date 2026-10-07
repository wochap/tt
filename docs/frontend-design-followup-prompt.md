# Follow-up brief for Claude Design: tt web GUI, round 2

Paste everything below this line into the same Claude Design project.

---

The first round covers every screen and both flavors. This round fixes consistency problems and fills the gaps listed below. Keep all existing frames; add or revise only what is named here.

## 1. Make Latte a first-class theme

Latte currently exists only as an inline `.ctp-latte` override that remaps about twelve Nocturne tokens. The ramps 200–700 and 900 still hold dark values, and `--color-accent-100` is remapped to a saturated accent, which inverts its meaning. Fix:

- Define both flavors as complete token sets with the same variable names: every `--color-*` ramp step, surface, divider and shadow has a Mocha and a Latte value. Nothing in a frame may depend on a dark-only value.
- Remove all hard-coded hex values and raw pixel radii from frames. Canvas labels, frame shadows and monospace font declarations must go through tokens. Frame shadows for Latte must be tuned for a light ground, not reused from Mocha.
- Produce a token sheet frame showing both flavors side by side, including a small-text contrast check.

## 2. Fix small-text contrast

Text at 10–12px using `--c-ov0` and `--c-ov1` fails 4.5:1 in Latte and partly in Mocha (hour labels, hints, status bar, "key/value" row, "hold ⌥ to free"). Pick ramp steps that pass AA for small text in both flavors, or raise the size. Show the before/after on the Day frame.

## 3. Reconcile numbers

Several totals are literals that do not match the computed data: week summed 29h 06m versus rows that sum to 31h 09m, overlap, per-project footer, "+1h 01m" strip caption, "Running · 2", "Raw · 7 entries". Every total, badge and caption must be derived from the same data as the entries it summarizes.

## 4. Fill missing states and frames

- **Empty states**: brand-new user on Day, Week, Month, Tasks and Reports. Include one-line guidance and a primary action (create first task, start tracking).
- **Drag-create release**: draw the anchored task picker that appears when the user releases a drag on empty space, with fuzzy results, the "create new task" row, and keyboard navigation hints.
- **Jump to date**: a popover with a calendar and a natural-language input (`next mon`, `2026-10-03`, `-3d`).
- **Snap grid chip**: a menu on the timeline chip to switch 5/10/15 minutes, plus the modifier hint.
- **Week view re-link**: show the task side panel or an equivalent drop target in Week view, matching Day view.
- **Task detail, non-running state**: a "Start tracking" primary action, and the entries list with an "all time" scope toggle. Add the markdown Edit mode (editor with preview toggle), not only Preview.
- **Fuzzy search results**: Tasks list with a query typed (`PROJ-123`) showing matched ticket metadata highlighted.
- **Reports custom range**: date and optional time pickers for start and end. Show the "per task per day" grouping variant of the rounded preview next to the "per entry" one.
- **Board view** for Tasks, columns by state, cards draggable between columns.
- **Phone Day view totals**: summed and wall-clock, collapsible.
- **Shortcut sheet** opened with `?`, and a single hover / focus / pressed state sheet for buttons, entries, rows and chips in both flavors.

## 5. Component inventory

Replace the single paragraph with a table: each UI element mapped to its shadcn/ui primitive (Button, Sheet, Dialog, Popover, Calendar, Command, Select, Tabs, Switch, ToggleGroup, Badge, Table, Tooltip, Toast), the custom components that have no shadcn equivalent (timeline grid, entry block, lane layout, running strip, round/flatten preview), and the props each custom component needs.

## 6. Document shortcut scoping

`S` means start on Day and Tasks but stop in the entry sheet; `J`/`K` move rows in lists but switch tasks in detail. Either unify them or document the scoping explicitly in the shortcut sheet. Keep the quick-create syntax `title +tag @project key:value`; it is accepted as an extension of the brief.

## 7. Minor

- Running counters in the strip should follow the project color precedence (project color, then tag color, then neutral) like entry blocks do, instead of always green.
- Fix the bundle README path: files live under `design/project/`, not `catppuccin-theme-implementation/project/`.
