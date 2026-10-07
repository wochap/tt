import { describe, expect, it } from "vitest";

import { newEntry } from "@tt/domain";

import { type GridMetrics, preview } from "./drag.ts";
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
});
