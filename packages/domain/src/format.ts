// Display formats shared by the UI: durations, clocks and date labels.

import type { Millis } from "./model.ts";
import { type CivilDate, isoWeek, localParts, weekday } from "./time.ts";

const pad = (n: number) => String(n).padStart(2, "0");

/** `1h 05m`, `45m`, `0m` (seconds in, minutes truncated). */
export function formatDuration(seconds: number): string {
  const minutes = Math.floor(Math.max(0, seconds) / 60);
  const hours = Math.floor(minutes / 60);
  return hours > 0 ? `${hours}h ${pad(minutes % 60)}m` : `${minutes}m`;
}

/** `1h05m` / `25m` like `tt-core::report::format_seconds`. */
export function formatSecondsCompact(seconds: number): string {
  const minutes = Math.trunc(seconds / 60);
  const hours = Math.trunc(minutes / 60);
  return hours > 0 ? `${hours}h${pad(minutes % 60)}m` : `${minutes}m`;
}

/** Running counter `1:12:08`. */
export function formatElapsed(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 3600)}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
}

/** Local `HH:MM`. */
export function formatClock(tz: string, at: Millis): string {
  const p = localParts(tz, at);
  return `${pad(p.hour)}:${pad(p.minute)}`;
}

export const DAY_NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as const;
export const DAY_NAMES_LONG = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"] as const;
export const MONTH_NAMES = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"] as const;
export const MONTH_NAMES_LONG = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
] as const;

/** `Thu 8 Oct`. */
export function formatDayShort(date: CivilDate): string {
  return `${DAY_NAMES[weekday(date)]} ${date.day} ${MONTH_NAMES[date.month - 1]}`;
}

/** `Thursday, 8 October`. */
export function formatDayLong(date: CivilDate): string {
  return `${DAY_NAMES_LONG[weekday(date)]}, ${date.day} ${MONTH_NAMES_LONG[date.month - 1]}`;
}

/** `5 – 11 October` or `28 Sep – 4 Oct`. */
export function formatSpan(from: CivilDate, to: CivilDate, long = true): string {
  const names = long ? MONTH_NAMES_LONG : MONTH_NAMES;
  if (from.year === to.year && from.month === to.month) return `${from.day} – ${to.day} ${names[to.month - 1]}`;
  return `${from.day} ${MONTH_NAMES[from.month - 1]} – ${to.day} ${MONTH_NAMES[to.month - 1]}`;
}

export function formatWeekLabel(date: CivilDate): string {
  return `W${isoWeek(date).week}`;
}
