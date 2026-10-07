// Drag state machine of the grid (dnd-kit supplies activation, the 4 px
// threshold, pointer capture and drop targets; times come from the pointer
// position over the column rects). Snap: start snaps down and end up on
// create; move rounds the start; Alt frees to 1 min; Shift locks the column.

import { diffDays, type Entry, formatClock, formatDuration, localDate, localParts, type Millis, type Uuid } from "@tt/domain";

import { type Column, fromWall, snapCeil, snapFloor, snapRound, wallMinutes } from "./geometry.ts";

export type DragKind = "create" | "move" | "resize-start" | "resize-end";

export interface Pointer {
  x: number;
  y: number;
  alt: boolean;
  shift: boolean;
}

export interface GridMetrics {
  tz: string;
  columns: Column[];
  firstHour: number;
  pxPerHour: number;
  /** Column element rects, same order as `columns`. */
  rects: () => DOMRect[];
  snapMinutes: number;
  now: Millis;
}

export interface DragOrigin {
  kind: DragKind;
  pointer: Pointer;
  column: Column;
  entry?: Entry;
}

export interface CreatePreview {
  kind: "create";
  column: Column;
  start: Millis;
  end: Millis;
  /** Raw pointer span in minutes (a drag below 5 min is a click). */
  rawMinutes: number;
}

export interface EntryPreview {
  kind: "move" | "resize-start" | "resize-end";
  entry: Entry;
  start: Millis;
  /** `null` keeps a running entry running. */
  end: Millis | null;
  deltaMinutes: number;
  /** Hovered side-panel task (re-link drop target). */
  overTask?: Uuid;
}

export type Preview = CreatePreview | EntryPreview;

export function gridOf(pointer: Pointer, metrics: GridMetrics): number {
  return pointer.alt || metrics.snapMinutes <= 0 ? 1 : metrics.snapMinutes;
}

function minutesAt(metrics: GridMetrics, columnIndex: number, y: number): number {
  const rect = metrics.rects()[columnIndex];
  if (!rect) return 0;
  return metrics.firstHour * 60 + ((y - rect.top) / metrics.pxPerHour) * 60;
}

function columnIndexAt(metrics: GridMetrics, x: number, fallback: number): number {
  const rects = metrics.rects();
  const index = rects.findIndex((r) => x >= r.left && x < r.right);
  if (index >= 0) return index;
  if (!rects.length) return fallback;
  return x < rects[0]!.left ? 0 : rects.length - 1;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function preview(origin: DragOrigin, pointer: Pointer, metrics: GridMetrics, overTask?: Uuid): Preview {
  const { tz } = metrics;
  const grid = gridOf(pointer, metrics);
  const originIndex = Math.max(0, metrics.columns.findIndex((c) => c.key === origin.column.key));

  if (origin.kind === "create") {
    const anchor = minutesAt(metrics, originIndex, origin.pointer.y);
    const current = minutesAt(metrics, originIndex, pointer.y);
    const lo = clamp(Math.min(anchor, current), 0, 1440);
    const hi = clamp(Math.max(anchor, current), 0, 1440);
    const startMin = clamp(snapFloor(lo, grid), 0, 1440 - grid);
    const endMin = clamp(Math.max(snapCeil(hi, grid), startMin + grid), grid, 1440);
    return {
      kind: "create",
      column: origin.column,
      start: fromWall(tz, origin.column.date, startMin),
      end: fromWall(tz, origin.column.date, endMin),
      rawMinutes: hi - lo,
    };
  }

  const entry = origin.entry!;
  const end = entry.end ?? metrics.now;
  const minDuration = grid * 60_000;

  if (origin.kind === "move") {
    if (overTask) return { kind: "move", entry, start: entry.start, end: entry.end, deltaMinutes: 0, overTask };
    const deltaY = pointer.y - origin.pointer.y;
    const deltaMinutes = (deltaY / metrics.pxPerHour) * 60;
    const targetIndex = pointer.shift ? originIndex : columnIndexAt(metrics, pointer.x, originIndex);
    const target = metrics.columns[targetIndex] ?? origin.column;
    const days = diffDays(target.date, origin.column.date);
    const startOwn = startMinutes(tz, origin.column, entry.start);
    const startMin = snapRound(startOwn + deltaMinutes, grid);
    let start = fromWall(tz, origin.column.date, startMin + days * 1440);
    if (entry.end === null) {
      // Running entries move their start only; the end stays "now".
      start = Math.min(start, metrics.now - 60_000);
      return { kind: "move", entry, start, end: null, deltaMinutes: Math.round((start - entry.start) / 60_000) };
    }
    return { kind: "move", entry, start, end: start + (end - entry.start), deltaMinutes: Math.round((start - entry.start) / 60_000) };
  }

  const pointerMin = minutesAt(metrics, originIndex, pointer.y);
  const at = fromWall(tz, origin.column.date, clamp(snapRound(pointerMin, grid), 0, 1440));
  if (origin.kind === "resize-start") {
    const start = Math.min(at, end - minDuration);
    return { kind: "resize-start", entry, start, end: entry.end, deltaMinutes: Math.round((start - entry.start) / 60_000) };
  }
  // Bottom handle: a running entry is stopped at the dropped time.
  const newEnd = clamp(at, entry.start + minDuration, entry.end === null ? metrics.now : Infinity);
  return { kind: "resize-end", entry, start: entry.start, end: Math.max(newEnd, entry.start + 60_000), deltaMinutes: Math.round((newEnd - end) / 60_000) };
}

/** Minutes of `at` past the midnight of `column` (negative before it, > 1440 after). */
export function startMinutes(tz: string, column: Column, at: Millis): number {
  if (at >= column.from && at < column.to) return wallMinutes(tz, column, at);
  const p = localParts(tz, at);
  return p.hour * 60 + p.minute + p.second / 60 + diffDays(localDate(tz, at), column.date) * 1440;
}

export function deltaLabel(minutes: number): string | undefined {
  if (!minutes) return undefined;
  return `${minutes > 0 ? "+" : "−"}${formatDuration(Math.abs(minutes) * 60)}`;
}

export function rangeLabel(tz: string, start: Millis, end: Millis | null): string {
  return `${formatClock(tz, start)} – ${end === null ? "now" : formatClock(tz, end)}`;
}
