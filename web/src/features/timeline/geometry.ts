// Pure geometry of the day/week grid: columns are local days; vertical
// position is local wall-clock minutes (DST-safe: a 23 h day still draws
// 00:00–24:00 labels and converts back through the zone).

import {
  addDays,
  type CivilDate,
  type Entry,
  formatDate,
  laneLayout,
  localDate,
  localParts,
  localToUtc,
  type Millis,
  startOfDay,
} from "@tt/domain";

export interface Column {
  date: CivilDate;
  key: string;
  from: Millis;
  to: Millis;
}

export function columnFor(tz: string, date: CivilDate): Column {
  return { date, key: formatDate(date), from: startOfDay(tz, date), to: startOfDay(tz, addDays(date, 1)) };
}

export function columnsFrom(tz: string, first: CivilDate, count: number): Column[] {
  return Array.from({ length: count }, (_, i) => columnFor(tz, addDays(first, i)));
}

/** Wall-clock minutes of `at` within `column` (0 at its midnight, 1440 at the next). */
export function wallMinutes(tz: string, column: Column, at: Millis): number {
  if (at <= column.from) return 0;
  if (at >= column.to) return 1440;
  const p = localParts(tz, at);
  return p.hour * 60 + p.minute + p.second / 60 + (at % 1000) / 60000;
}

/** Instant at `minutes` past local midnight of `date` (minutes may leave [0, 1440)). */
export function fromWall(tz: string, date: CivilDate, minutes: number): Millis {
  const dayShift = Math.floor(minutes / 1440);
  const within = minutes - dayShift * 1440;
  const day = dayShift ? addDays(date, dayShift) : date;
  const whole = Math.floor(within);
  const seconds = Math.round((within - whole) * 60);
  return localToUtc(tz, { ...day, hour: Math.floor(whole / 60), minute: whole % 60, second: Math.min(59, seconds) });
}

export function snapFloor(minutes: number, grid: number): number {
  return grid > 0 ? Math.floor(minutes / grid) * grid : minutes;
}
export function snapCeil(minutes: number, grid: number): number {
  return grid > 0 ? Math.ceil(minutes / grid) * grid : minutes;
}
export function snapRound(minutes: number, grid: number): number {
  return grid > 0 ? Math.round(minutes / grid) * grid : minutes;
}

/** Hours drawn: the visible hours widened to every entry (and now) in the columns. */
export function renderedHours(
  tz: string,
  columns: Column[],
  entries: Iterable<Entry>,
  visible: [number, number],
  now: Millis,
): [number, number] {
  let [first, last] = visible;
  for (const column of columns) {
    for (const entry of entries) {
      const end = entry.end ?? now;
      if (end <= column.from || entry.start >= column.to) continue;
      first = Math.min(first, Math.floor(wallMinutes(tz, column, entry.start) / 60));
      last = Math.max(last, Math.ceil(wallMinutes(tz, column, end) / 60));
    }
    if (now >= column.from && now < column.to) {
      const m = wallMinutes(tz, column, now);
      first = Math.min(first, Math.floor(m / 60));
      last = Math.max(last, Math.ceil((m + 1) / 60));
    }
  }
  return [Math.max(0, first), Math.min(24, Math.max(last, first + 1))];
}

export interface Placed {
  entry: Entry;
  /** Clipped to the column. */
  start: Millis;
  end: Millis;
  top: number;
  height: number;
  lane: number;
  lanes: number;
  /** The entry continues before / after this column. */
  clippedStart: boolean;
  clippedEnd: boolean;
}

/** Lane layout of one column; running entries extend to `now`. */
export function placeColumn(
  tz: string,
  column: Column,
  entries: Entry[],
  now: Millis,
  firstHour: number,
  pxPerHour: number,
): Placed[] {
  const inColumn = entries.filter((e) => (e.end ?? now) > column.from && e.start < column.to);
  const lanes = laneLayout(
    inColumn.map((e) => ({ id: e.id, start: Math.max(e.start, column.from), end: Math.min(e.end ?? now, column.to) })),
    now,
  );
  const byId = new Map(inColumn.map((e) => [e.id, e]));
  return lanes.map((placement) => {
    const entry = byId.get(placement.id)!;
    const top = ((wallMinutes(tz, column, placement.start) - firstHour * 60) / 60) * pxPerHour;
    const bottom = ((wallMinutes(tz, column, placement.end) - firstHour * 60) / 60) * pxPerHour;
    return {
      entry,
      start: placement.start,
      end: placement.end,
      top,
      height: Math.max(bottom - top - 2, 6),
      lane: placement.lane,
      lanes: placement.lanes,
      clippedStart: entry.start < column.from,
      clippedEnd: (entry.end ?? now) > column.to,
    };
  });
}

export function todayIn(tz: string, now: Millis): CivilDate {
  return localDate(tz, now);
}
