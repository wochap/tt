// Time zones, durations, points in time and ranges, mirroring
// `tt-core/src/time.rs`. Zones are IANA names resolved through `Intl`.

import { DomainError, type Millis } from "./model.ts";

export const MINUTE = 60_000;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;
/** Earliest/latest instants used for unbounded ranges. */
export const MIN_TIME = -8_640_000_000_000_000;
export const MAX_TIME = 8_640_000_000_000_000;

/** A calendar date without a zone. `month` is 1-12. */
export interface CivilDate {
  year: number;
  month: number;
  day: number;
}

/** Local wall-clock date and time. */
export interface CivilDateTime extends CivilDate {
  hour: number;
  minute: number;
  second: number;
}

/** Half-open interval `[from, to)` in UTC milliseconds. */
export interface Range {
  from: Millis;
  to: Millis;
}

/** Monday = 0 … Sunday = 6 (chrono's `num_days_from_monday`). */
export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6;
export const WEEKDAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] as const;

function invalid(input: string, what: string): DomainError {
  return new DomainError("invalid", `cannot parse ${what} ${JSON.stringify(input)}`);
}

// ---------- zones ----------

const formatters = new Map<string, Intl.DateTimeFormat>();

function formatter(tz: string): Intl.DateTimeFormat {
  let f = formatters.get(tz);
  if (!f) {
    f = new Intl.DateTimeFormat("en-US", {
      timeZone: tz,
      hourCycle: "h23",
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
      second: "numeric",
      era: "short",
    });
    formatters.set(tz, f);
  }
  return f;
}

export function isValidTimeZone(tz: string): boolean {
  try {
    formatter(tz);
    return true;
  } catch {
    return false;
  }
}

/** Resolves a zone name; `local`/empty means the system zone; unknown means UTC. */
export function resolveTz(name: string | undefined | null, system?: string): string {
  const pick = (value: string) => {
    const clean = value.trim().replace(/^:/, "");
    return clean && isValidTimeZone(clean) ? clean : undefined;
  };
  const named = name && name.trim() && name.trim() !== "local" ? pick(name) : undefined;
  return named ?? (system ? pick(system) : undefined) ?? "UTC";
}

export function systemTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

/** Wall-clock fields of `at` in `tz`. */
export function localParts(tz: string, at: Millis): CivilDateTime {
  const parts = formatter(tz).formatToParts(new Date(at));
  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0);
  const era = parts.find((p) => p.type === "era")?.value;
  let year = get("year");
  if (era === "BC" || era === "B") year = 1 - year;
  return { year, month: get("month"), day: get("day"), hour: get("hour"), minute: get("minute"), second: get("second") };
}

function civilToMillis(c: CivilDateTime): Millis {
  const ms = Date.UTC(2000, c.month - 1, c.day, c.hour, c.minute, c.second);
  const d = new Date(ms);
  d.setUTCFullYear(c.year);
  return d.getTime();
}

/** Offset of `tz` at instant `at`, milliseconds east of UTC. */
export function tzOffset(tz: string, at: Millis): number {
  const wall = civilToMillis(localParts(tz, Math.floor(at / 1000) * 1000));
  return wall - Math.floor(at / 1000) * 1000;
}

/**
 * Resolves a local wall-clock time: the earlier instant on a DST overlap,
 * skipping forward an hour over a DST gap (as `tt-core` does).
 */
export function localToUtc(tz: string, local: CivilDateTime): Millis {
  const wall = civilToMillis(local);
  const resolved = resolveWall(tz, wall);
  if (resolved !== undefined) return resolved;
  return resolveWall(tz, wall + HOUR) ?? wall;
}

function resolveWall(tz: string, wall: Millis): Millis | undefined {
  const offsets = new Set<number>();
  for (const probe of [-DAY, -HOUR * 12, 0, HOUR * 12, DAY]) offsets.add(tzOffset(tz, wall + probe));
  const valid = [...offsets].map((offset) => wall - offset).filter((utc) => tzOffset(tz, utc) === wall - utc);
  return valid.length ? Math.min(...valid) : undefined;
}

export function localDate(tz: string, at: Millis): CivilDate {
  const { year, month, day } = localParts(tz, at);
  return { year, month, day };
}

/** Local midnight at the start of `date`. */
export function startOfDay(tz: string, date: CivilDate): Millis {
  return localToUtc(tz, { ...date, hour: 0, minute: 0, second: 0 });
}

// ---------- civil date arithmetic ----------

function dayNumber(date: CivilDate): number {
  return Math.round(civilToMillis({ ...date, hour: 0, minute: 0, second: 0 }) / DAY);
}

function fromDayNumber(n: number): CivilDate {
  const d = new Date(n * DAY);
  return { year: d.getUTCFullYear(), month: d.getUTCMonth() + 1, day: d.getUTCDate() };
}

export function addDays(date: CivilDate, days: number): CivilDate {
  return fromDayNumber(dayNumber(date) + days);
}

export function addMonths(date: CivilDate, months: number): CivilDate {
  const index = date.year * 12 + (date.month - 1) + months;
  const year = Math.floor(index / 12);
  const month = index - year * 12 + 1;
  return { year, month, day: Math.min(date.day, daysInMonth(year, month)) };
}

export function daysInMonth(year: number, month: number): number {
  if (month === 2) return isLeap(year) ? 29 : 28;
  return [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1] ?? 30;
}

function isLeap(year: number): boolean {
  return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
}

export function diffDays(a: CivilDate, b: CivilDate): number {
  return dayNumber(a) - dayNumber(b);
}

export function compareDates(a: CivilDate, b: CivilDate): number {
  return dayNumber(a) - dayNumber(b);
}

export function sameDate(a: CivilDate, b: CivilDate): boolean {
  return a.year === b.year && a.month === b.month && a.day === b.day;
}

/** Monday = 0 … Sunday = 6. */
export function weekday(date: CivilDate): Weekday {
  return ((dayNumber(date) % 7) + 7 + 3) % 7 as Weekday;
}

export function weekStartDate(date: CivilDate, weekStart: Weekday): CivilDate {
  return addDays(date, -((7 + weekday(date) - weekStart) % 7));
}

export function monthStart(date: CivilDate): CivilDate {
  return { year: date.year, month: date.month, day: 1 };
}

/** ISO 8601 week number and week-based year. */
export function isoWeek(date: CivilDate): { year: number; week: number } {
  const thursday = addDays(date, 3 - weekday(date));
  const jan1 = { year: thursday.year, month: 1, day: 1 };
  return { year: thursday.year, week: Math.floor(diffDays(thursday, jan1) / 7) + 1 };
}

/** Monday of ISO week `week` in ISO year `year`. */
export function isoWeekStart(year: number, week: number): CivilDate {
  const jan4 = { year, month: 1, day: 4 };
  return addDays(addDays(jan4, -weekday(jan4)), (week - 1) * 7);
}

export function formatDate(date: CivilDate): string {
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${date.year < 0 ? "-" : ""}${pad(Math.abs(date.year), 4)}-${pad(date.month)}-${pad(date.day)}`;
}

export function dayRange(tz: string, from: CivilDate, toExclusive: CivilDate): Range {
  return { from: startOfDay(tz, from), to: startOfDay(tz, toExclusive) };
}

export function rangeOverlaps(range: Range, start: Millis, end: Millis): boolean {
  return start < range.to && end > range.from;
}

// ---------- parsing ----------

/** Parses a duration: `90` (minutes), `15m`, `1h30m`, `2h`, `45s`, `1d`. Returns milliseconds. */
export function parseDuration(input: string): number {
  const text = input.trim().toLowerCase();
  if (!text) throw invalid(input, "duration");
  if (/^[+-]?\d+$/.test(text)) return Number(text) * MINUTE;
  let total = 0;
  let number = "";
  let any = false;
  for (const ch of text) {
    if (ch >= "0" && ch <= "9") {
      number += ch;
      continue;
    }
    if (!number) throw invalid(input, "duration");
    const value = Number(number);
    number = "";
    const unit = { d: DAY, h: HOUR, m: MINUTE, s: 1000 }[ch];
    if (unit === undefined) throw invalid(input, "duration");
    total += value * unit;
    any = true;
  }
  if (number || !any) throw invalid(input, "duration");
  return total;
}

export function parseClock(input: string): { hour: number; minute: number; second: number } | undefined {
  const m = /^(\d{1,2}):(\d{1,2})(?::(\d{1,2}))?$/.exec(input);
  if (!m) return undefined;
  const hour = Number(m[1]);
  const minute = Number(m[2]);
  const second = m[3] === undefined ? 0 : Number(m[3]);
  if (hour > 23 || minute > 59 || second > 59) return undefined;
  return { hour, minute, second };
}

export function parseDate(input: string): CivilDate | undefined {
  const m = /^([+-]?\d{4,})-(\d{1,2})-(\d{1,2})$/.exec(input);
  if (!m) return undefined;
  const date = { year: Number(m[1]), month: Number(m[2]), day: Number(m[3]) };
  if (date.month < 1 || date.month > 12 || date.day < 1 || date.day > daysInMonth(date.year, date.month)) {
    return undefined;
  }
  return date;
}

const RFC3339 =
  /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(\.\d+)?([Zz]|[+-]\d{2}:\d{2})$/;

function parseRfc3339(text: string): Millis | undefined {
  const m = RFC3339.exec(text);
  if (!m) return undefined;
  const date = parseDate(`${m[1]}-${m[2]}-${m[3]}`);
  const clock = parseClock(`${m[4]}:${m[5]}:${m[6]}`);
  if (!date || !clock) return undefined;
  const fraction = m[7] ? Math.floor(Number(`0${m[7]}`) * 1000) : 0;
  let offset = 0;
  if (m[8] && m[8].toUpperCase() !== "Z") {
    const sign = m[8][0] === "-" ? -1 : 1;
    offset = sign * (Number(m[8].slice(1, 3)) * HOUR + Number(m[8].slice(4, 6)) * MINUTE);
  }
  return civilToMillis({ ...date, ...clock }) + fraction - offset;
}

/**
 * Parses a point in time: `now`, `-30m`/`+1h` (relative to now), `13:00`
 * (today), `yesterday 13:00`, `today 9:15`, `tomorrow 8:00`, `2026-10-07`,
 * `2026-10-07 13:00`, `2026-10-07T13:00[:SS]`, or RFC 3339 with an offset.
 */
export function parseTime(input: string, now: Millis, tz: string): Millis {
  const text = input.trim();
  const lower = text.toLowerCase();
  if (lower === "now") return now;
  if (lower.startsWith("-")) return now - parseDuration(lower.slice(1));
  if (lower.startsWith("+")) return now + parseDuration(lower.slice(1));
  const rfc = parseRfc3339(text);
  if (rfc !== undefined) return rfc;
  const today = localDate(tz, now);
  const parts = text.split(/\s+/).filter(Boolean);
  if (parts.length > 2) throw invalid(input, "time");
  const [first = "", second] = parts;
  const named: Record<string, CivilDate> = { today, yesterday: addDays(today, -1), tomorrow: addDays(today, 1) };
  const day = named[first.toLowerCase()] ?? parseDate(first);
  if (day) {
    const clock = second === undefined ? { hour: 0, minute: 0, second: 0 } : parseClock(second);
    if (!clock) throw invalid(input, "time");
    return localToUtc(tz, { ...day, ...clock });
  }
  if (second === undefined) {
    const clock = parseClock(first);
    if (clock) return localToUtc(tz, { ...today, ...clock });
    const m = /^([+-]?\d{4,}-\d{1,2}-\d{1,2})T(\d{1,2}:\d{1,2}(?::\d{1,2})?)$/.exec(first.replace(/t/g, "T"));
    if (m) {
      const date = parseDate(m[1]!);
      const time = parseClock(m[2]!);
      if (date && time) return localToUtc(tz, { ...date, ...time });
    }
  }
  throw invalid(input, "time");
}

function parseBound(input: string, now: Millis, tz: string, end: boolean): Millis {
  const text = input.trim();
  const date = parseDate(text);
  if (date) return startOfDay(tz, end ? addDays(date, 1) : date);
  const lower = text.toLowerCase();
  if (end && lower === "today") return startOfDay(tz, addDays(localDate(tz, now), 1));
  if (end && lower === "yesterday") return startOfDay(tz, localDate(tz, now));
  return parseTime(text, now, tz);
}

/**
 * Parses a range: `today`/`day`, `yesterday`, `week`, `lastweek`, `month`,
 * `lastmonth`, `year`, `all`, a single `YYYY-MM-DD`, or `<from>..<to>` where
 * either side is a date or a time. An empty side means unbounded start or now.
 */
export function parseRange(input: string, now: Millis, tz: string, weekStart: Weekday): Range {
  const text = input.trim();
  const today = localDate(tz, now);
  let range: Range;
  switch (text.toLowerCase()) {
    case "today":
    case "day":
      range = dayRange(tz, today, addDays(today, 1));
      break;
    case "yesterday":
      range = dayRange(tz, addDays(today, -1), today);
      break;
    case "week": {
      const start = weekStartDate(today, weekStart);
      range = dayRange(tz, start, addDays(start, 7));
      break;
    }
    case "lastweek":
    case "last-week": {
      const start = addDays(weekStartDate(today, weekStart), -7);
      range = dayRange(tz, start, addDays(start, 7));
      break;
    }
    case "month": {
      const start = monthStart(today);
      range = dayRange(tz, start, addMonths(start, 1));
      break;
    }
    case "lastmonth":
    case "last-month": {
      const start = addMonths(monthStart(today), -1);
      range = dayRange(tz, start, monthStart(today));
      break;
    }
    case "year":
      range = dayRange(tz, { year: today.year, month: 1, day: 1 }, { year: today.year + 1, month: 1, day: 1 });
      break;
    case "all":
      range = { from: MIN_TIME, to: MAX_TIME };
      break;
    default: {
      const split = text.indexOf("..");
      if (split >= 0) {
        const from = text.slice(0, split);
        const to = text.slice(split + 2);
        range = {
          from: from.trim() ? parseBound(from, now, tz, false) : MIN_TIME,
          to: to.trim() ? parseBound(to, now, tz, true) : now,
        };
      } else {
        const date = parseDate(text);
        if (!date) throw invalid(input, "range");
        range = dayRange(tz, date, addDays(date, 1));
      }
    }
  }
  if (range.from >= range.to) throw new DomainError("invalid", `range ${JSON.stringify(input)} is empty`);
  return range;
}

export function parseWeekday(input: string): Weekday {
  const text = input.trim().toLowerCase();
  const full = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
  const index = WEEKDAYS.findIndex((day, i) => text === day || text === full[i]);
  if (index < 0) throw invalid(input, "weekday");
  return index as Weekday;
}

const MONTHS = "january february march april may june july august september october november december".split(" ");

function monthIndex(word: string): number | undefined {
  const w = word.toLowerCase().replace(/\.$/, "");
  if (w.length < 3) return undefined;
  const index = MONTHS.findIndex((month) => month.startsWith(w));
  return index >= 0 ? index + 1 : undefined;
}

function weekdayWord(word: string): Weekday | undefined {
  const w = word.toLowerCase().replace(/\.$/, "");
  if (w.length < 2) return undefined;
  const full = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
  const index = full.findIndex((day) => day.startsWith(w));
  return index >= 0 ? (index as Weekday) : undefined;
}

/**
 * Natural-language date for "jump to date": `today`, `yesterday`, `tomorrow`,
 * `-3d` / `+2w` / `+1m`, `2026-10-03`, `w42` (ISO week, this year), `next mon`,
 * `last fri`, `this wed`, `mon` (this week), `12 oct`, `oct 12`, `12 october 2026`.
 */
export function parseJumpDate(input: string, today: CivilDate, weekStart: Weekday = 0): CivilDate {
  const text = input.trim().toLowerCase().replace(/\s+/g, " ");
  if (!text) throw invalid(input, "date");
  if (text === "today" || text === "now") return today;
  if (text === "yesterday") return addDays(today, -1);
  if (text === "tomorrow") return addDays(today, 1);
  const rel = /^([+-])(\d+)\s*(d|day|days|w|wk|week|weeks|m|mo|month|months|y|year|years)?$/.exec(text);
  if (rel) {
    const n = Number(rel[2]) * (rel[1] === "-" ? -1 : 1);
    const unit = rel[3]?.[0] ?? "d";
    if (unit === "d") return addDays(today, n);
    if (unit === "w") return addDays(today, n * 7);
    if (unit === "m") return addMonths(today, n);
    return addMonths(today, n * 12);
  }
  const iso = parseDate(text);
  if (iso) return iso;
  const week = /^(?:w|week|kw|cw)\s?(\d{1,2})(?:\s+(\d{4}))?$/.exec(text);
  if (week) {
    const year = week[2] ? Number(week[2]) : isoWeek(today).year;
    const n = Number(week[1]);
    if (n >= 1 && n <= 53) return isoWeekStart(year, n);
  }
  const words = text.split(" ");
  if (words.length <= 2) {
    const [a = "", b] = words;
    const day = weekdayWord(b ?? a);
    if (day !== undefined && (b === undefined || ["next", "last", "this", "prev", "previous"].includes(a))) {
      const thisWeek = addDays(weekStartDate(today, weekStart), (7 + day - weekStart) % 7);
      if (b === undefined || a === "this") return thisWeek;
      if (a === "next") {
        const delta = (7 + day - weekday(today)) % 7 || 7;
        return addDays(today, delta);
      }
      const delta = (7 + weekday(today) - day) % 7 || 7;
      return addDays(today, -delta);
    }
  }
  const dm = /^(\d{1,2})\.?\s*([a-z]+\.?)(?:,?\s*(\d{4}))?$/.exec(text) ?? null;
  const md = /^([a-z]+\.?)\s*(\d{1,2})(?:,?\s*(\d{4}))?$/.exec(text) ?? null;
  const pick = dm
    ? { day: Number(dm[1]), month: monthIndex(dm[2]!), year: dm[3] }
    : md
      ? { day: Number(md[2]), month: monthIndex(md[1]!), year: md[3] }
      : undefined;
  if (pick?.month) {
    const year = pick.year ? Number(pick.year) : today.year;
    if (pick.day >= 1 && pick.day <= daysInMonth(year, pick.month)) return { year, month: pick.month, day: pick.day };
  }
  const numeric = /^(\d{1,2})[./](\d{1,2})(?:[./](\d{4}))?\.?$/.exec(text);
  if (numeric) {
    const year = numeric[3] ? Number(numeric[3]) : today.year;
    const day = Number(numeric[1]);
    const month = Number(numeric[2]);
    if (month >= 1 && month <= 12 && day >= 1 && day <= daysInMonth(year, month)) return { year, month, day };
  }
  throw invalid(input, "date");
}
