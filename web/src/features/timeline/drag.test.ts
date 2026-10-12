import { describe, expect, it, vi } from "vitest";

import { newEntry } from "@tt/domain";

import { edgeScrollSpeed, type GridMetrics, minutesOfDayAt, pointFromEvent, preview, pulseIfTouch } from "./drag.ts";
import { columnsFrom } from "./geometry.ts";

const tz = "Europe/Berlin";
const day = { year: 2026, month: 10, day: 8 };
const columns = columnsFrom(tz, day, 3);
const PX = 56;
const FIRST = 7;
// Columns 300 px wide side by side; grid top at y = 100.
const metrics = (snap = 15): GridMetrics => ({
  tz,
  columns,
  firstHour: FIRST,
  pxPerHour: PX,
  rects: () => columns.map((_, i) => new DOMRect(100 + i * 300, 100, 300, 17 * PX)),
  snapMinutes: snap,
  now: Date.UTC(2026, 9, 8, 13, 42),
});
const y = (h: number, m: number) => 100 + (h - FIRST + m / 60) * PX;
const p = (x: number, yy: number, alt = false, shift = false) => ({ x, y: yy, alt, shift });
const local = (h: number, m: number, d = 8) => Date.UTC(2026, 9, d, h - 2, m); // CEST = UTC+2

describe("drag preview", () => {
  it("create: 13:07 → 13:52 with a 15 min snap proposes 13:00–14:00", () => {
    const origin = { kind: "create" as const, column: columns[0]!, pointer: p(150, y(13, 7)) };
    const result = preview(origin, p(150, y(13, 52)), metrics());
    expect(result).toMatchObject({ kind: "create", start: local(13, 0), end: local(14, 0) });
  });

  it("create: dragging upward flips the anchor", () => {
    const origin = { kind: "create" as const, column: columns[0]!, pointer: p(150, y(13, 52)) };
    const result = preview(origin, p(150, y(13, 7)), metrics());
    expect(result).toMatchObject({ start: local(13, 0), end: local(14, 0) });
  });

  it("create: Alt frees the snap to one minute", () => {
    const origin = { kind: "create" as const, column: columns[0]!, pointer: p(150, y(13, 7)) };
    const result = preview(origin, p(150, y(13, 52), true), metrics());
    expect(result).toMatchObject({ start: local(13, 7), end: local(13, 52) });
  });

  it("create: the anchor stays fixed when the column rect shifts by a scroll offset", () => {
    const m = metrics();
    const anchorMinutes = minutesOfDayAt(m, columns[0]!, y(13, 7));
    const origin = { kind: "create" as const, column: columns[0]!, pointer: p(150, y(13, 7)), anchorMinutes };
    // Scrolled 224 px (4 h) up: the finger is still at the same screen y, 4 h further down the content.
    const scrolled: GridMetrics = { ...m, rects: () => columns.map((_, i) => new DOMRect(100 + i * 300, 100 - 224, 300, 17 * PX)) };
    const result = preview(origin, p(150, y(13, 7)), scrolled);
    expect(result).toMatchObject({ start: local(13, 0), end: local(17, 15) });
  });

  describe("pointFromEvent", () => {
    it("reads mouse/pointer events with modifiers", () => {
      const e = new MouseEvent("mousedown", { clientX: 12, clientY: 34, altKey: true });
      expect(pointFromEvent(e)).toEqual({ x: 12, y: 34, alt: true, shift: false });
    });

    it("reads the first touch and ignores modifiers", () => {
      const e = { touches: [{ clientX: 5, clientY: 6 }], changedTouches: [], altKey: true, shiftKey: true } as unknown as TouchEvent;
      expect(pointFromEvent(e)).toEqual({ x: 5, y: 6, alt: false, shift: false });
      const end = { touches: [], changedTouches: [{ clientX: 7, clientY: 8 }] } as unknown as TouchEvent;
      expect(pointFromEvent(end)).toEqual({ x: 7, y: 8, alt: false, shift: false });
    });
  });

  it("pulseIfTouch vibrates for touch only", () => {
    const vibrate = vi.fn();
    Object.defineProperty(navigator, "vibrate", { value: vibrate, configurable: true });
    pulseIfTouch({ touches: [] } as unknown as TouchEvent);
    expect(vibrate).toHaveBeenCalledWith(10);
    vibrate.mockClear();
    pulseIfTouch(new MouseEvent("mousedown"));
    expect(vibrate).not.toHaveBeenCalled();
  });

  describe("edgeScrollSpeed", () => {
    const rect = { top: 100, bottom: 1100, height: 1000 };
    it("is 0 outside the zone and at its boundary", () => {
      expect(edgeScrollSpeed(600, rect)).toBe(0);
      expect(edgeScrollSpeed(220, rect)).toBe(0);
      expect(edgeScrollSpeed(980, rect)).toBe(0);
    });
    it("is signed and proportional inside the zone, capped at the edge", () => {
      expect(edgeScrollSpeed(160, rect, 0.12, 20)).toBeCloseTo(-10);
      expect(edgeScrollSpeed(1040, rect, 0.12, 20)).toBeCloseTo(10);
      expect(edgeScrollSpeed(100, rect, 0.12, 20)).toBe(-20);
      expect(edgeScrollSpeed(2000, rect, 0.12, 20)).toBe(20);
    });
  });

  const entry = newEntry("018f0000-0000-7000-8000-0000000000c1", local(10, 0), local(10, 45), undefined, 0);

  it("move: keeps the duration, snaps the start, crosses days unless Shift", () => {
    const origin = { kind: "move" as const, column: columns[0]!, entry, pointer: p(150, y(10, 10)) };
    const down = preview(origin, p(150, y(10, 38)), metrics());
    expect(down).toMatchObject({ start: local(10, 30), end: local(11, 15), deltaMinutes: 30 });
    const nextDay = preview(origin, p(450, y(10, 10)), metrics());
    expect(nextDay).toMatchObject({ start: local(10, 0, 9), end: local(10, 45, 9) });
    const locked = preview(origin, p(450, y(10, 10), false, true), metrics());
    expect(locked).toMatchObject({ start: local(10, 0) });
  });

  it("move over a side-panel task re-links without moving", () => {
    const origin = { kind: "move" as const, column: columns[0]!, entry, pointer: p(150, y(10, 10)) };
    expect(preview(origin, p(999, y(12, 0)), metrics(), "task-30")).toMatchObject({ start: entry.start, end: entry.end, overTask: "task-30" });
  });

  it("resize: one grid step minimum; a running entry's bottom edge stops it, capped at now", () => {
    const top = preview({ kind: "resize-start", column: columns[0]!, entry, pointer: p(150, y(10, 0)) }, p(150, y(10, 50)), metrics());
    expect(top).toMatchObject({ start: local(10, 30), end: entry.end });
    const running = { ...entry, end: null };
    const bottom = preview({ kind: "resize-end", column: columns[0]!, entry: running, pointer: p(150, y(13, 42)) }, p(150, y(18, 0)), metrics());
    expect(bottom).toMatchObject({ end: Date.UTC(2026, 9, 8, 13, 42) });
  });

  describe("pending range", () => {
    const range = { start: local(13, 0), end: local(14, 0) };
    const base = { column: columns[0]!, range };

    it("move by 30 min snaps and keeps the duration", () => {
      const origin = { ...base, kind: "pending-move" as const, pointer: p(150, y(13, 10)) };
      expect(preview(origin, p(150, y(13, 42)), metrics())).toMatchObject({ kind: "pending-move", start: local(13, 30), end: local(14, 30) });
    });

    it("move: Shift locks the column, otherwise it crosses days", () => {
      const origin = { ...base, kind: "pending-move" as const, pointer: p(150, y(13, 10)) };
      expect(preview(origin, p(450, y(13, 10)), metrics())).toMatchObject({ column: columns[1], start: local(13, 0, 9) });
      expect(preview(origin, p(450, y(13, 10), false, true), metrics())).toMatchObject({ column: columns[0], start: local(13, 0) });
    });

    it("resize bottom to 14:55 gives 13:00–15:00", () => {
      const origin = { ...base, kind: "pending-resize-end" as const, pointer: p(150, y(14, 0)) };
      expect(preview(origin, p(150, y(14, 55)), metrics())).toMatchObject({ start: local(13, 0), end: local(15, 0) });
    });

    it("resize top below the bottom clamps to one grid step", () => {
      const origin = { ...base, kind: "pending-resize-start" as const, pointer: p(150, y(13, 0)) };
      expect(preview(origin, p(150, y(16, 0)), metrics())).toMatchObject({ start: local(13, 45), end: local(14, 0) });
    });

    it("Alt frees the snap to one minute", () => {
      const origin = { ...base, kind: "pending-resize-end" as const, pointer: p(150, y(14, 0)) };
      expect(preview(origin, p(150, y(14, 50), true), metrics())).toMatchObject({ end: local(14, 50) });
    });
  });
});
