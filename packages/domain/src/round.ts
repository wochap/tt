// Rounding and flattening for billing-style exports, mirroring
// `tt-core/src/round.rs`. Each item's duration is rounded to the duration
// grid and its start snapped to the start grid on the local wall clock, then
// items are laid out in order of original start so none overlap:
// `start = max(snapped start, previous end)`, `end = start + rounded`. With
// `task-day` grouping, durations are first summed per (task, local day) and
// each group starts at its first entry's start, snapped.

import { compare, DomainError, type Millis, type Uuid } from "./model.ts";
import { seconds } from "./intervals.ts";
import { formatDate, localDate, tzOffset } from "./time.ts";

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
  /** First raw start of the item. */
  raw_start: Millis;
  /** Start snapped to the start grid, before the push. */
  grid_start: Millis;
  /** Local day (`YYYY-MM-DD`) of `raw_start`. */
  day: string;
  /** Rounded to 0: left out of downloads, does not move the cursor. */
  dropped: boolean;
}

/** Grids (seconds) and modes for `roundAndFlatten`; a zero start grid is off. */
export interface RoundOptions {
  gridSeconds: number;
  mode: RoundMode;
  startGridSeconds: number;
  startMode: RoundMode;
  group: RoundGroup;
}

/** Start grids offered besides off, minutes. */
export const START_GRID_MINUTES = [5, 10, 15, 30, 60] as const;

/** Start grids (seconds, 0 = off) allowed with a `durationSeconds` grid. */
export function validStartGrids(durationSeconds: number): number[] {
  return [0, ...START_GRID_MINUTES.map((m) => m * 60).filter((g) => durationSeconds > 0 && durationSeconds % g === 0)];
}

function gridLabel(secs: number): string {
  return secs === 0 ? "off" : `${secs / 60}m`;
}

/** Rejects a start grid that does not divide the duration grid. */
export function validateGrids(options: Pick<RoundOptions, "gridSeconds" | "startGridSeconds">): void {
  const valid = validStartGrids(options.gridSeconds);
  if (valid.includes(options.startGridSeconds)) return;
  const start = options.startGridSeconds;
  throw new DomainError(
    "invalid",
    `start grid ${start % 60 === 0 ? gridLabel(start) : `${start}s`} does not divide the duration grid ${gridLabel(options.gridSeconds)}; valid start grids: ${valid.map(gridLabel).join(", ")}`,
  );
}

function divEuclid(a: number, b: number): number {
  return Math.floor(a / b);
}

/** Rounds `secs` to `grid` seconds. */
export function roundSeconds(secs: number, grid: number, mode: RoundMode): number {
  if (grid <= 0) return secs;
  return mode === "up" ? divEuclid(secs + grid - 1, grid) * grid : divEuclid(secs + Math.trunc(grid / 2), grid) * grid;
}

/**
 * Snaps `at` to a `grid`-second mark of the local wall clock, using the zone
 * offset at `at` (never converting a local time back).
 */
export function snapStart(at: Millis, grid: number, mode: RoundMode, tz: string): Millis {
  if (grid <= 0) return at;
  const offset = tzOffset(tz, at) / 1000;
  const r = (((Math.floor(at / 1000) + offset) % grid) + grid) % grid;
  const delta = r === 0 ? 0 : mode === "nearest" && 2 * r < grid ? -r : grid - r;
  return at + delta * 1000;
}

interface Prepared {
  start: Millis;
  task: Uuid;
  seconds: number;
  entries: Uuid[];
  day: string;
}

/**
 * Rounds and flattens `items`; output order equals input order by start.
 * Dropped (0-length) items stay in the output, flagged. Throws
 * `DomainError("invalid")` for an invalid grid pair.
 */
export function roundAndFlatten(items: readonly RoundInput[], options: RoundOptions, tz: string): Rounded[] {
  validateGrids(options);
  let prepared: Prepared[];
  if (options.group === "entry") {
    prepared = items.map((item) => ({
      start: item.start,
      task: item.task,
      seconds: Math.max(0, seconds(item.start, item.end)),
      entries: [...item.entries],
      day: formatDate(localDate(tz, item.start)),
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
    const duration = roundSeconds(item.seconds, options.gridSeconds, options.mode);
    const gridStart = snapStart(item.start, options.startGridSeconds, options.startMode, tz);
    const dropped = duration === 0;
    const start = dropped || cursor === undefined ? gridStart : Math.max(cursor, gridStart);
    const end = start + duration * 1000;
    if (!dropped) cursor = end;
    return {
      task: item.task,
      start,
      end,
      duration,
      original: item.seconds,
      entries: item.entries,
      raw_start: item.start,
      grid_start: gridStart,
      day: item.day,
      dropped,
    };
  });
}
