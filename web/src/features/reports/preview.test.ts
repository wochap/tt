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
    const rounded = roundAndFlatten(inputs, { gridSeconds: 900, mode: "up", startGridSeconds: 0, startMode: "up", group: "entry" }, "UTC");
    const { raw, flat } = previewRows(inputs, rounded);
    expect(flat.map((r) => [r.start, r.end])).toEqual(rounded.map((r) => [r.start, r.end]));
    expect(flat.map((r) => [r.start, r.end])).toEqual([
      [t(13, 0), t(13, 15)],
      [t(13, 15), t(14, 0)],
    ]);
    expect(flat.map((r) => r.badge)).toEqual(["rounded", "moved"]);
    expect(raw.map((r) => r.seconds)).toEqual([300, 2100]);
  });

  it("badges a snapped start: 21:22–21:37 nearest → 21:15–21:30", () => {
    const t = (h: number, m: number) => Date.UTC(2026, 9, 7, h, m);
    const inputs = [{ task: "a", start: t(21, 22), end: t(21, 37), entries: ["e1"] }];
    const rounded = roundAndFlatten(inputs, { gridSeconds: 900, mode: "nearest", startGridSeconds: 900, startMode: "nearest", group: "entry" }, "UTC");
    const { flat } = previewRows(inputs, rounded);
    expect(flat.map((r) => [r.start, r.end, r.badge])).toEqual([[t(21, 15), t(21, 30), "snapped"]]);
  });

  it("keeps a dropped row opposite its raw row", () => {
    const t = (h: number, m: number) => Date.UTC(2026, 9, 7, h, m);
    const inputs = [
      { task: "a", start: t(9, 0), end: t(9, 4), entries: ["e1"] },
      { task: "a", start: t(9, 5), end: t(9, 20), entries: ["e2"] },
    ];
    const rounded = roundAndFlatten(inputs, { gridSeconds: 900, mode: "nearest", startGridSeconds: 0, startMode: "nearest", group: "entry" }, "UTC");
    const { raw, flat } = previewRows(inputs, rounded);
    expect(flat).toHaveLength(raw.length);
    expect(flat.map((r) => r.badge)).toEqual(["dropped", undefined]);
    expect(flat[1]!.start).toBe(t(9, 5));
  });
});
