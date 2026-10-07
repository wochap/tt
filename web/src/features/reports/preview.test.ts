import { describe, expect, it } from "vitest";

import { roundAndFlatten } from "@tt/domain";

import { previewRows } from "./RoundFlattenPreview.tsx";

describe("round + flatten preview", () => {
  it("equals the domain output: 13:00–13:05 and 13:10–13:45 → 13:00–13:15 and 13:15–14:00", () => {
    const t = (h: number, m: number) => Date.UTC(2026, 9, 7, h, m);
    const inputs = [
      { task: "a", start: t(13, 0), end: t(13, 5), entries: ["e1"] },
      { task: "a", start: t(13, 10), end: t(13, 45), entries: ["e2"] },
    ];
    const rounded = roundAndFlatten(inputs, 900, "up", "entry", "UTC");
    const { raw, flat } = previewRows(inputs, rounded);
    expect(flat.map((r) => [r.start, r.end])).toEqual(rounded.map((r) => [r.start, r.end]));
    expect(flat.map((r) => [r.start, r.end])).toEqual([
      [t(13, 0), t(13, 15)],
      [t(13, 15), t(14, 0)],
    ]);
    expect(flat.map((r) => r.badge)).toEqual(["rounded", "moved"]);
    expect(raw.map((r) => r.seconds)).toEqual([300, 2100]);
  });
});
