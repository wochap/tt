// Drag state machine of the grid (dnd-kit supplies activation, the 4 px
// threshold, pointer capture and drop targets; times come from the pointer
// position over the column rects). Snap: start snaps down and end up on
// create; move rounds the start; Alt frees to 1 min; Shift locks the column.

import { diffDays, type Entry, formatClock, formatDuration, localDate, localParts, type Millis, type Uuid } from "@tt/domain";

import { type Column, fromWall, snapCeil, snapFloor, snapRound, wallMinutes } from "./geometry.ts";

export type PendingKind = "pending-move" | "pending-resize-start" | "pending-resize-end";
export type DragKind = "create" | "move" | "resize-start" | "resize-end" | PendingKind;

export interface Span {
  start: Millis;
  end: Millis;
}

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
  /** Create only: the pressed time in minutes of day, fixed at drag start so scrolling cannot shift it. */
  anchorMinutes?: number;
  entry?: Entry;
  /** The pending (dotted) range being adjusted, for the `pending-*` kinds. */
  range?: Span;
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

/** Adjusted pending range (create-shaped: there is no entry behind it). */
export interface PendingPreview {
  kind: PendingKind;
  column: Column;
  start: Millis;
  end: Millis;
}

export type Preview = CreatePreview | EntryPreview | PendingPreview;

export function isEntryPreview(p: Preview): p is EntryPreview {
  return p.kind === "move" || p.kind === "resize-start" || p.kind === "resize-end";
}

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

/** Minutes of day under `y` in the given column (the create anchor, captured at drag start). */
export function minutesOfDayAt(metrics: GridMetrics, column: Column, y: number): number {
  return minutesAt(metrics, Math.max(0, metrics.columns.findIndex((c) => c.key === column.key)), y);
}

/** Position and modifiers of a mouse/pointer or touch event (touch: first touch, no modifiers). */
export function pointFromEvent(e: MouseEvent | PointerEvent | TouchEvent): Pointer {
  if ("touches" in e) {
    const t = e.touches[0] ?? e.changedTouches[0];
    return { x: t?.clientX ?? 0, y: t?.clientY ?? 0, alt: false, shift: false };
  }
  return { x: e.clientX, y: e.clientY, alt: e.altKey, shift: e.shiftKey };
}

/** Short haptic pulse when a touch hold arms a drag (no-op for mouse and where unsupported). */
export function pulseIfTouch(e: Event): void {
  if ("touches" in e) navigator.vibrate?.(10);
}

/**
 * Auto-scroll speed (px per frame) for a pointer at `pointerY` over the scroller `rect`:
 * 0 outside the edge zone (`zone` = fraction of the height), negative near the top,
 * positive near the bottom, proportional to the depth into the zone.
 */
export function edgeScrollSpeed(pointerY: number, rect: { top: number; bottom: number; height: number }, zone = 0.12, max = 18): number {
  const size = rect.height * zone;
  if (size <= 0) return 0;
  const top = pointerY - rect.top;
  const bottom = rect.bottom - pointerY;
  if (top < size) return -max * Math.min(1, (size - top) / size);
  if (bottom < size) return max * Math.min(1, (size - bottom) / size);
  return 0;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function preview(origin: DragOrigin, pointer: Pointer, metrics: GridMetrics, overTask?: Uuid): Preview {
  const { tz } = metrics;
  const grid = gridOf(pointer, metrics);
  const originIndex = Math.max(0, metrics.columns.findIndex((c) => c.key === origin.column.key));

  if (origin.kind === "create") {
    const anchor = origin.anchorMinutes ?? minutesAt(metrics, originIndex, origin.pointer.y);
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

  if (origin.range) {
    const span = origin.range;
    if (origin.kind === "pending-move") {
      const moved = movedSpan(origin, span, pointer, metrics, grid, originIndex);
      return { kind: "pending-move", column: moved.column, start: moved.start, end: moved.end };
    }
    const at = pointerTime(origin, pointer, metrics, grid, originIndex);
    if (origin.kind === "pending-resize-start") {
      return { kind: "pending-resize-start", column: origin.column, start: Math.min(at, span.end - grid * 60_000), end: span.end };
    }
    return { kind: "pending-resize-end", column: origin.column, start: span.start, end: Math.max(at, span.start + grid * 60_000) };
  }

  const entry = origin.entry!;
  const end = entry.end ?? metrics.now;
  const minDuration = grid * 60_000;

  if (origin.kind === "move") {
    if (overTask) return { kind: "move", entry, start: entry.start, end: entry.end, deltaMinutes: 0, overTask };
    const moved = movedSpan(origin, { start: entry.start, end }, pointer, metrics, grid, originIndex);
    if (entry.end === null) {
      // Running entries move their start only; the end stays "now".
      const start = Math.min(moved.start, metrics.now - 60_000);
      return { kind: "move", entry, start, end: null, deltaMinutes: Math.round((start - entry.start) / 60_000) };
    }
    return { kind: "move", entry, start: moved.start, end: moved.end, deltaMinutes: Math.round((moved.start - entry.start) / 60_000) };
  }

  const at = pointerTime(origin, pointer, metrics, grid, originIndex);
  if (origin.kind === "resize-start") {
    const start = Math.min(at, end - minDuration);
    return { kind: "resize-start", entry, start, end: entry.end, deltaMinutes: Math.round((start - entry.start) / 60_000) };
  }
  // Bottom handle: a running entry is stopped at the dropped time.
  const newEnd = clamp(at, entry.start + minDuration, entry.end === null ? metrics.now : Infinity);
  return { kind: "resize-end", entry, start: entry.start, end: Math.max(newEnd, entry.start + 60_000), deltaMinutes: Math.round((newEnd - end) / 60_000) };
}

/** Snapped time under the pointer in the origin column (resize edges). */
function pointerTime(origin: DragOrigin, pointer: Pointer, metrics: GridMetrics, grid: number, originIndex: number): Millis {
  const pointerMin = minutesAt(metrics, originIndex, pointer.y);
  return fromWall(metrics.tz, origin.column.date, clamp(snapRound(pointerMin, grid), 0, 1440));
}

/** A span moved by the pointer delta: start snapped, duration kept, column from the pointer unless Shift. */
function movedSpan(origin: DragOrigin, span: Span, pointer: Pointer, metrics: GridMetrics, grid: number, originIndex: number): Span & { column: Column } {
  const { tz } = metrics;
  const deltaMinutes = ((pointer.y - origin.pointer.y) / metrics.pxPerHour) * 60;
  const targetIndex = pointer.shift ? originIndex : columnIndexAt(metrics, pointer.x, originIndex);
  const column = metrics.columns[targetIndex] ?? origin.column;
  const days = diffDays(column.date, origin.column.date);
  const startMin = snapRound(startMinutes(tz, origin.column, span.start) + deltaMinutes, grid);
  const start = fromWall(tz, origin.column.date, startMin + days * 1440);
  return { column, start, end: start + (span.end - span.start) };
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
