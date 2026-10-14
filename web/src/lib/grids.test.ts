import { describe, expect, it } from "vitest";

import { roundSpan, startOptions, withDuration } from "./grids.ts";

describe("grids", () => {
  it("disables start grids that do not divide the duration", () => {
    expect(startOptions(15)).toEqual([0, 5, 15]);
    expect(startOptions(10)).toEqual([0, 5, 10]);
    expect(startOptions(30)).toEqual([0, 5, 10, 15, 30]);
    expect(startOptions(60)).toEqual([0, 5, 10, 15, 30, 60]);
    expect(startOptions(5)).toEqual([0, 5]);
  });

  it("falls back to the largest valid start", () => {
    expect(withDuration({ start: 15, duration: 15 }, 10)).toEqual({ start: 10, duration: 10 });
    expect(withDuration({ start: 30, duration: 30 }, 15)).toEqual({ start: 15, duration: 15 });
    expect(withDuration({ start: 0, duration: 15 }, 5)).toEqual({ start: 0, duration: 5 });
    expect(withDuration({ start: 5, duration: 15 }, 60)).toEqual({ start: 5, duration: 60 });
  });
});

describe("roundSpan", () => {
  const at = (h: number, m: number) => Date.UTC(2026, 9, 8, h, m);
  it("floors the start to the start grid and rounds the length up to the duration grid", () => {
    expect(roundSpan(at(9, 7), at(9, 25), { start: 15, duration: 30 })).toEqual({ start: at(9, 0), end: at(9, 30) });
  });
  it("keeps the start to the minute when off and leaves a running end alone", () => {
    expect(roundSpan(at(9, 7) + 20_000, at(9, 25), { start: 0, duration: 15 })).toEqual({ start: at(9, 7), end: at(9, 22) + 15 * 60_000 });
    expect(roundSpan(at(9, 7), null, { start: 15, duration: 30 })).toEqual({ start: at(9, 0) });
  });
});
