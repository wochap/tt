// Interval union and the summed / wall-clock / overlap totals, mirroring
// `union_seconds` and `clip` in `tt-core/src/report.rs`.

import type { Entry, Millis } from "./model.ts";
import type { Range } from "./time.ts";

export type Interval = [start: Millis, end: Millis];

/** Total length in seconds of the union of `[start, end)` intervals. */
export function unionSeconds(intervals: readonly Interval[]): number {
  const sorted = intervals.filter(([s, e]) => e > s).sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  let total = 0;
  let current: Interval | undefined;
  for (const [start, end] of sorted) {
    if (current && start <= current[1]) {
      if (end > current[1]) current[1] = end;
    } else {
      if (current) total += seconds(current[0], current[1]);
      current = [start, end];
    }
  }
  if (current) total += seconds(current[0], current[1]);
  return total;
}

/** Whole seconds between two instants (truncated like chrono's `num_seconds`). */
export function seconds(start: Millis, end: Millis): number {
  return Math.trunc((end - start) / 1000);
}

/** Entry clipped to `range`, with running entries ending at `now`. */
export function clip(entry: Entry, range: Range, now: Millis): Interval | undefined {
  const start = Math.max(entry.start, range.from);
  const end = Math.min(entry.end ?? now, range.to);
  return end > start ? [start, end] : undefined;
}

export interface Totals {
  /** Σ of every clipped duration, seconds. */
  summed: number;
  /** Length of the union, seconds. */
  wall: number;
  /** `summed - wall`: time counted more than once. */
  overlap: number;
  count: number;
}

/** Totals over entries clipped to `range` (all of time when omitted). */
export function totals(entries: Iterable<Entry>, now: Millis, range?: Range): Totals {
  const intervals: Interval[] = [];
  for (const entry of entries) {
    const interval = range ? clip(entry, range, now) : clip(entry, { from: -Infinity, to: Infinity }, now);
    if (interval) intervals.push(interval);
  }
  const summed = intervals.reduce((sum, [s, e]) => sum + seconds(s, e), 0);
  const wall = unionSeconds(intervals);
  return { summed, wall, overlap: summed - wall, count: intervals.length };
}
