// Preview model of the rounded & flattened export (Turn 9 frame 10, 9.1–9.5):
// raw rows paired with rounded rows, grouped under local days and, past 7
// days, followed by week subtotal rows. Pure; the component only renders it.

import {
  addDays,
  type CivilDate,
  diffDays,
  formatDate,
  formatDayShort,
  formatSpan,
  formatWeekLabel,
  localDate,
  type Millis,
  parseDate,
  type Range,
  type RoundGroup,
  type RoundInput,
  type Rounded,
  type Weekday,
  weekStartDate,
} from "@tt/domain";

export type Badge = "moved" | "snapped" | "rounded" | "dropped";

export interface RawRow {
  key: string;
  task: string;
  start: Millis;
  end: Millis;
  seconds: number;
}

export interface FlatRow {
  key: string;
  task: string;
  start: Millis;
  end: Millis;
  seconds: number;
  /** Source entries (task/day rows list several). */
  count: number;
  badges: Badge[];
  /** Ends on a later local day than the day it is listed under. */
  plusDay: boolean;
  dropped: boolean;
}

/** One table row: raw half (left) and rounded half (right); either may be padding. */
export interface PairRow {
  key: string;
  raw?: RawRow;
  flat?: FlatRow;
}

export interface DaySection {
  kind: "day";
  /** `YYYY-MM-DD`. */
  day: string;
  label: string;
  rawSub: number;
  flatSub: number;
  rows: PairRow[];
}

export interface WeekSection {
  kind: "week";
  key: string;
  label: string;
  range: string;
  rawSub: number;
  flatSub: number;
}

export type Section = DaySection | WeekSection;

export interface PreviewModel {
  sections: Section[];
  /** Day headers shown (range spans more than one local day). */
  multiDay: boolean;
  rawTotal: number;
  flatTotal: number;
  /** Rows in the download (not dropped) and dropped rows. */
  downloadRows: number;
  droppedRows: number;
}

/** Badges of a rounded row, derived from the domain output alone. */
export function badgesOf(item: Rounded): Badge[] {
  if (item.dropped) return ["dropped"];
  const badges: Badge[] = [];
  if (item.start > item.grid_start) badges.push("moved");
  else if (item.grid_start !== item.raw_start) badges.push("snapped");
  if (item.duration !== item.original) badges.push("rounded");
  return badges;
}

const secondsOf = (start: Millis, end: Millis) => Math.max(0, Math.trunc((end - start) / 1000));

export function rawRows(inputs: readonly RoundInput[]): RawRow[] {
  return inputs.map((item) => ({ key: item.entries[0] ?? "", task: item.task, start: item.start, end: item.end, seconds: secondsOf(item.start, item.end) }));
}

export function flatRows(rounded: readonly Rounded[], tz: string): FlatRow[] {
  return rounded.map((item) => ({
    key: item.entries[0] ?? "",
    task: item.task,
    start: item.start,
    end: item.end,
    seconds: item.duration,
    count: item.entries.length,
    badges: badgesOf(item),
    plusDay: !item.dropped && item.end > item.start && formatDate(localDate(tz, item.end - 1)) > item.day,
    dropped: item.dropped,
  }));
}

export function previewModel(
  inputs: readonly RoundInput[],
  rounded: readonly Rounded[],
  group: RoundGroup,
  tz: string,
  weekStart: Weekday,
  range: Range,
): PreviewModel {
  const raw = rawRows(inputs);
  const flat = flatRows(rounded, tz);
  const rawDay = (r: RawRow) => formatDate(localDate(tz, r.start));
  const flatDay = rounded.map((r) => r.day);

  const byDay = new Map<string, PairRow[]>();
  const rowsOf = (day: string) => {
    let rows = byDay.get(day);
    if (!rows) byDay.set(day, (rows = []));
    return rows;
  };
  if (group === "entry") {
    // Rounded output order equals input order: pair by index.
    raw.forEach((r, i) => rowsOf(rawDay(r)).push({ key: `${r.key}-${r.start}`, raw: r, flat: flat[i] }));
  } else {
    const rawBy = new Map<string, RawRow[]>();
    const flatBy = new Map<string, FlatRow[]>();
    for (const r of raw) rawBy.set(rawDay(r), [...(rawBy.get(rawDay(r)) ?? []), r]);
    flat.forEach((f, i) => flatBy.set(flatDay[i]!, [...(flatBy.get(flatDay[i]!) ?? []), f]));
    for (const day of new Set([...rawBy.keys(), ...flatBy.keys()])) {
      const left = rawBy.get(day) ?? [];
      const right = flatBy.get(day) ?? [];
      for (let i = 0; i < Math.max(left.length, right.length); i++) rowsOf(day).push({ key: `${day}-${i}`, raw: left[i], flat: right[i] });
    }
  }

  const days = [...byDay.keys()].sort();
  const first = localDate(tz, range.from);
  const last = localDate(tz, range.to - 1);
  const span = diffDays(last, first) + 1;
  const multiDay = span > 1;
  const weeks = span > 7;

  const sum = (rows: PairRow[], side: "raw" | "flat") =>
    rows.reduce((total, row) => total + (side === "raw" ? (row.raw?.seconds ?? 0) : row.flat && !row.flat.dropped ? row.flat.seconds : 0), 0);
  const daySection = (day: string): DaySection => {
    const rows = byDay.get(day)!;
    return { kind: "day", day, label: formatDayShort(parseDate(day)!), rawSub: sum(rows, "raw"), flatSub: sum(rows, "flat"), rows };
  };

  const sections: Section[] = [];
  let week: { start: CivilDate; raw: number; flat: number } | undefined;
  const closeWeek = () => {
    if (!week) return;
    sections.push({
      kind: "week",
      key: `week-${formatDate(week.start)}`,
      label: formatWeekLabel(week.start),
      range: formatSpan(week.start, addDays(week.start, 6), false),
      rawSub: week.raw,
      flatSub: week.flat,
    });
  };
  for (const day of days) {
    const section = daySection(day);
    if (weeks) {
      const start = weekStartDate(parseDate(day)!, weekStart);
      if (!week || formatDate(week.start) !== formatDate(start)) {
        closeWeek();
        week = { start, raw: 0, flat: 0 };
      }
      week.raw += section.rawSub;
      week.flat += section.flatSub;
    }
    sections.push(section);
  }
  if (weeks) closeWeek();

  const dropped = flat.filter((f) => f.dropped).length;
  return {
    sections,
    multiDay,
    rawTotal: raw.reduce((a, r) => a + r.seconds, 0),
    flatTotal: flat.reduce((a, r) => a + (r.dropped ? 0 : r.seconds), 0),
    downloadRows: flat.length - dropped,
    droppedRows: dropped,
  };
}
