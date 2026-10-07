// Derived reads over the merged view, shared by lists, pickers and reports.

import { clip, type Entry, type Millis, type Range, seconds, type Task, type Uuid, type View } from "@tt/domain";

/** Latest entry start per task. */
export function lastTracked(view: View): Map<Uuid, Millis> {
  const out = new Map<Uuid, Millis>();
  for (const entry of view.entries.values()) {
    const at = entry.end ?? Number.MAX_SAFE_INTEGER;
    if ((out.get(entry.task) ?? -Infinity) < at) out.set(entry.task, at);
  }
  return out;
}

/** Tracked seconds per task within `range` (all time when omitted). */
export function taskTotals(view: View, now: Millis, range?: Range): Map<Uuid, number> {
  const bounds = range ?? { from: -Infinity, to: Infinity };
  const out = new Map<Uuid, number>();
  for (const entry of view.entries.values()) {
    const interval = clip(entry, bounds, now);
    if (interval) out.set(entry.task, (out.get(entry.task) ?? 0) + seconds(interval[0], interval[1]));
  }
  return out;
}

export function runningTaskIds(view: View): Set<Uuid> {
  const out = new Set<Uuid>();
  for (const entry of view.entries.values()) if (entry.end === null) out.add(entry.task);
  return out;
}

/** Tasks for pickers: running first, then most recently tracked, then by seq. Archived excluded. */
export function pickerTasks(view: View): Task[] {
  const recent = lastTracked(view);
  return [...view.workspace.tasks.values()]
    .filter((task) => task.state !== "archived")
    .sort(
      (a, b) =>
        Number(b.state === "open") - Number(a.state === "open") ||
        (recent.get(b.id) ?? -Infinity) - (recent.get(a.id) ?? -Infinity) ||
        b.seq - a.seq,
    );
}

/** Entries overlapping `range`, start order. */
export function entriesInRange(view: View, range: Range, now: Millis): Entry[] {
  return [...view.entries.values()]
    .filter((entry) => entry.start < range.to && (entry.end ?? now) > range.from)
    .sort((a, b) => a.start - b.start || (a.id < b.id ? -1 : 1));
}
