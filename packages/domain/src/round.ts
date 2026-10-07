// Rounding and flattening for billing-style exports, mirroring
// `tt-core/src/round.rs`. Each item's duration is rounded to the grid, then
// items are laid out in order of original start so none overlap:
// `start = max(own start, previous end)`, `end = start + rounded`. With
// `task-day` grouping, durations are first summed per (task, local day) and
// each group starts at its first entry's start.

import { compare, DomainError, type Millis, type Uuid } from "./model.ts";
import { seconds } from "./intervals.ts";
import { formatDate, localDate } from "./time.ts";

export type RoundMode = "up" | "nearest";
export type RoundGroup = "entry" | "task-day";

export function parseRoundMode(value: string): RoundMode {
  const mode = value.trim().toLowerCase();
  if (mode === "up" || mode === "nearest") return mode;
  throw new DomainError("invalid", `unknown rounding mode ${JSON.stringify(mode)} (up, nearest)`);
}

export function parseRoundGroup(value: string): RoundGroup {
  const group = value.trim().toLowerCase();
  if (group === "entry") return "entry";
  if (group === "task-day" || group === "taskday" || group === "task_day") return "task-day";
  throw new DomainError("invalid", `unknown rounding group ${JSON.stringify(group)} (entry, task-day)`);
}

export interface RoundInput {
  task: Uuid;
  start: Millis;
  end: Millis;
  /** Source entry ids (one for `entry` grouping). */
  entries: Uuid[];
}

export interface Rounded {
  task: Uuid;
  start: Millis;
  end: Millis;
  /** Rounded duration, seconds. */
  duration: number;
  /** Original (unrounded) duration, seconds. */
  original: number;
  entries: Uuid[];
  /** Local day (`YYYY-MM-DD`) for `task-day` groups. */
  day?: string;
}

function divEuclid(a: number, b: number): number {
  return Math.floor(a / b);
}

/** Rounds `secs` to `grid` seconds. */
export function roundSeconds(secs: number, grid: number, mode: RoundMode): number {
  if (grid <= 0) return secs;
  return mode === "up" ? divEuclid(secs + grid - 1, grid) * grid : divEuclid(secs + Math.trunc(grid / 2), grid) * grid;
}

interface Prepared {
  start: Millis;
  task: Uuid;
  seconds: number;
  entries: Uuid[];
  day?: string;
}

/** Rounds and flattens `items`; output order equals input order by start. */
export function roundAndFlatten(
  items: readonly RoundInput[],
  gridSeconds: number,
  mode: RoundMode,
  group: RoundGroup,
  tz: string,
): Rounded[] {
  let prepared: Prepared[];
  if (group === "entry") {
    prepared = items.map((item) => ({
      start: item.start,
      task: item.task,
      seconds: Math.max(0, seconds(item.start, item.end)),
      entries: [...item.entries],
    }));
  } else {
    const groups = new Map<string, Prepared>();
    for (const item of items) {
      const day = formatDate(localDate(tz, item.start));
      const key = `${item.task}\u0000${day}`;
      const slot = groups.get(key) ?? { start: item.start, task: item.task, seconds: 0, entries: [], day };
      slot.start = Math.min(slot.start, item.start);
      slot.seconds += Math.max(0, seconds(item.start, item.end));
      slot.entries.push(...item.entries);
      groups.set(key, slot);
    }
    // BTreeMap order in tt-core: (task, day); the stable sort below keeps it for ties.
    prepared = [...groups.entries()]
      .sort(([a], [b]) => compare(a, b))
      .map(([, slot]) => slot);
  }
  prepared.sort((a, b) => a.start - b.start || compare(a.task, b.task));
  let cursor: Millis | undefined;
  return prepared.map((item) => {
    const duration = roundSeconds(item.seconds, gridSeconds, mode);
    const start = cursor === undefined ? item.start : Math.max(cursor, item.start);
    const end = start + duration * 1000;
    cursor = end;
    return {
      task: item.task,
      start,
      end,
      duration,
      original: item.seconds,
      entries: item.entries,
      ...(item.day !== undefined ? { day: item.day } : {}),
    };
  });
}
