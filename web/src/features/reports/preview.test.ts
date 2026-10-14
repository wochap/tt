import { describe, expect, it } from "vitest";

import { type RoundInput, type RoundOptions, roundAndFlatten } from "@tt/domain";

import { type DaySection, previewModel, type WeekSection } from "./preview.ts";

const tz = "UTC";
const t = (d: number, h: number, m: number) => Date.UTC(2026, 9, d, h, m);
const dayRange = (d: number) => ({ from: t(d, 0, 0), to: t(d + 1, 0, 0) });
const options = (o: Partial<RoundOptions> = {}): RoundOptions => ({ gridSeconds: 900, mode: "up", startGridSeconds: 0, startMode: "up", group: "entry", ...o });
const model = (inputs: RoundInput[], o: Partial<RoundOptions>, range = dayRange(7), weekStart: 0 | 6 = 0) => {
  const opts = options(o);
  const rounded = roundAndFlatten(inputs, opts, tz);
  return { rounded, m: previewModel(inputs, rounded, opts.group, tz, weekStart, range) };
};
const days = (m: ReturnType<typeof previewModel>) => m.sections.filter((s): s is DaySection => s.kind === "day");
const weeks = (m: ReturnType<typeof previewModel>) => m.sections.filter((s): s is WeekSection => s.kind === "week");

describe("preview model", () => {
  it("single day: one section, no header, pairs equal the domain output", () => {
    const inputs = [
      { task: "a", start: t(7, 13, 0), end: t(7, 13, 5), entries: ["e1"] },
      { task: "a", start: t(7, 13, 10), end: t(7, 13, 45), entries: ["e2"] },
    ];
    const { rounded, m } = model(inputs, {});
    expect(m.multiDay).toBe(false);
    expect(m.sections).toHaveLength(1);
    const rows = days(m)[0]!.rows;
    expect(rows.map((r) => [r.flat!.start, r.flat!.end])).toEqual(rounded.map((r) => [r.start, r.end]));
    expect(rows.map((r) => [r.flat!.start, r.flat!.end])).toEqual([
      [t(7, 13, 0), t(7, 13, 15)],
      [t(7, 13, 15), t(7, 14, 0)],
    ]);
    expect(rows.map((r) => r.flat!.badges)).toEqual([["rounded"], ["moved", "rounded"]]);
    expect(rows.map((r) => r.raw!.seconds)).toEqual([300, 2100]);
  });

  it("badges a snapped start: 21:22–21:37 nearest → 21:15–21:30", () => {
    const { m } = model([{ task: "a", start: t(7, 21, 22), end: t(7, 21, 37), entries: ["e1"] }], { mode: "nearest", startGridSeconds: 900, startMode: "nearest" });
    const row = days(m)[0]!.rows[0]!.flat!;
    expect([row.start, row.end, row.badges]).toEqual([t(7, 21, 15), t(7, 21, 30), ["snapped"]]);
  });

  it("counts dropped rows and keeps them opposite their raw row", () => {
    const inputs = [
      { task: "a", start: t(7, 9, 0), end: t(7, 9, 4), entries: ["e1"] },
      { task: "a", start: t(7, 9, 5), end: t(7, 9, 20), entries: ["e2"] },
    ];
    const { m } = model(inputs, { mode: "nearest", startMode: "nearest" });
    const rows = days(m)[0]!.rows;
    expect(rows[0]!.raw!.key).toBe("e1");
    expect(rows[0]!.flat).toMatchObject({ dropped: true, badges: ["dropped"] });
    expect(m.downloadRows).toBe(1);
    expect(m.droppedRows).toBe(1);
    expect(m.flatTotal).toBe(900);
  });

  it("week: Tue 23:55 is listed under Tue with +1d and pushes Wed's early row", () => {
    // 2026-10-05 is a Monday.
    const inputs = [
      { task: "a", start: t(6, 23, 55), end: t(7, 0, 20), entries: ["late"] },
      { task: "a", start: t(7, 0, 10), end: t(7, 0, 25), entries: ["early"] },
    ];
    const { m } = model(inputs, { mode: "nearest", startGridSeconds: 900, startMode: "nearest" }, { from: t(5, 0, 0), to: t(12, 0, 0) });
    expect(m.multiDay).toBe(true);
    expect(weeks(m)).toHaveLength(0);
    const [tue, wed] = days(m);
    expect(tue!.label).toBe("Tue 6 Oct");
    expect(tue!.rows[0]!.flat).toMatchObject({ start: t(7, 0, 0), end: t(7, 0, 30), plusDay: true });
    expect(wed!.rows[0]!.flat).toMatchObject({ start: t(7, 0, 30), badges: expect.arrayContaining(["moved"]), plusDay: false });
    expect(tue!.rawSub).toBe(25 * 60);
    expect(tue!.flatSub).toBe(30 * 60);
  });

  it("month: week subtotal rows follow each week, with a Sunday week start", () => {
    // 2026-10-03 Sat, 10-04 Sun, 10-05 Mon, 10-11 Sun.
    const inputs = [
      { task: "a", start: t(3, 9, 0), end: t(3, 10, 0), entries: ["sat"] },
      { task: "a", start: t(4, 9, 0), end: t(4, 9, 30), entries: ["sun"] },
      { task: "a", start: t(5, 9, 0), end: t(5, 9, 45), entries: ["mon"] },
      { task: "a", start: t(11, 9, 0), end: t(11, 9, 15), entries: ["sun2"] },
    ];
    const { m } = model(inputs, {}, { from: t(1, 0, 0), to: Date.UTC(2026, 10, 1) }, 6);
    expect(m.sections.map((s) => (s.kind === "day" ? s.day : s.kind))).toEqual(["2026-10-03", "week", "2026-10-04", "2026-10-05", "week", "2026-10-11", "week"]);
    expect(weeks(m).map((w) => w.rawSub)).toEqual([3600, 4500, 900]);
    expect(weeks(m)[1]!.label).toBe("W40");
  });

  it("per task / day pads the shorter side per day", () => {
    const inputs = [
      { task: "a", start: t(7, 9, 0), end: t(7, 9, 10), entries: ["e1"] },
      { task: "a", start: t(7, 10, 0), end: t(7, 10, 10), entries: ["e2"] },
      { task: "b", start: t(7, 11, 0), end: t(7, 11, 10), entries: ["e3"] },
      { task: "a", start: t(8, 9, 0), end: t(8, 9, 10), entries: ["e4"] },
    ];
    const opts = options({ group: "task-day" });
    const rounded = roundAndFlatten(
      [
        { task: "a", start: t(7, 9, 0), end: t(7, 10, 10), entries: ["e1", "e2"] },
        { task: "b", start: t(7, 11, 0), end: t(7, 11, 10), entries: ["e3"] },
        { task: "a", start: t(8, 9, 0), end: t(8, 9, 10), entries: ["e4"] },
      ],
      opts,
      tz,
    );
    const m = previewModel(inputs, rounded, "task-day", tz, 0, { from: t(5, 0, 0), to: t(12, 0, 0) });
    const [d7, d8] = days(m);
    expect(d7!.rows).toHaveLength(3);
    expect(d7!.rows.map((r) => !!r.flat)).toEqual([true, true, false]);
    expect(d8!.rows).toHaveLength(1);
    expect(d8!.rows[0]!.flat!.count).toBe(1);
  });
});
