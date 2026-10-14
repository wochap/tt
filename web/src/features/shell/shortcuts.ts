import { MOD } from "@/lib/utils";

// The shortcut sheet (frame 22): one key, one verb, scoped.
export const SHORTCUTS: { scope: string; keys: [string, string][] }[] = [
  {
    scope: "Global",
    keys: [
      [`${MOD}K`, "Command palette"],
      ["?", "This sheet"],
      [`${MOD}Z / ⇧${MOD}Z`, "Undo / redo (all edits)"],
      ["1 · 2 · 3", "Timeline · Tasks · Reports"],
      [",", "Settings"],
      ["⇧S", "Stop all running"],
      ["C", "New task (quick-create syntax)"],
      ["Esc", "Close sheet / popover"],
    ],
  },
  {
    scope: "Timeline",
    keys: [
      ["D · W · M", "Day · Week · Month"],
      ["← →", "Previous / next range"],
      ["T", "Today"],
      ["G", "Jump to date"],
      ["F · ⇧F", "Filter · clear filters"],
      ["J · K", "Next / previous entry"],
      ["⏎", "Open focused entry"],
      ["S", "Toggle tracking on focused entry’s task"],
      ["N", "New entry at now (opens picker)"],
      ["⌥ drag", "Disable snapping"],
      ["⇧ drag", "Lock to column"],
      ["V", "Select / Create mode"],
      ["Space", "Toggle focused entry (Select)"],
      [`${MOD}A`, "Select all visible entries"],
      ["⌫", "Delete selection (undoable)"],
      ["Esc", "Clear selection, then back to Create"],
    ],
  },
  {
    scope: "Reports",
    keys: [
      ["D · W · M", "Day · Week · Month"],
      ["← →", "Previous / next range"],
      ["F · ⇧F", "Filter · clear filters"],
    ],
  },
  {
    scope: "Lists (Tasks, side panel, reports)",
    keys: [
      ["J · K", "Next / previous row"],
      ["/", "Focus search"],
      ["⏎", "Open row"],
      ["S", "Toggle tracking on row"],
      ["E", "Rename inline"],
      ["X", "Mark done / reopen"],
      ["Space", "Toggle selection on row"],
      ["⇧J · ⇧K", "Extend selection"],
      [`${MOD}A`, "Select all in filter"],
      ["⌫", "Delete selection, or the focused task (undoable)"],
      [".", "Open row menu"],
      ["B · L", "Board / list view"],
    ],
  },
  {
    scope: "Entry sheet",
    keys: [
      ["S", "Toggle tracking (stop if running)"],
      ["⇧⏎", "Split at marker"],
      [`${MOD}M`, "Move to task"],
      ["⌫", "Delete (undoable)"],
      ["[ · ]", "Start −15m / +15m"],
      ["{ · }", "End −15m / +15m"],
    ],
  },
  {
    scope: "Task detail",
    keys: [
      ["J · K", "Next / previous task"],
      ["S", "Toggle tracking on this task"],
      ["E", "Edit title"],
      [`${MOD}E`, "Toggle description edit / preview"],
      ["X", "Mark done"],
      ["⌫", "Delete task (undoable)"],
    ],
  },
];
