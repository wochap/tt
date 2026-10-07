// Runs the shared fixtures under `fixtures/` (expected values come from
// tt-core; see fixtures/README.md).

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  type Entry,
  type GroupBy,
  type Project,
  type Rounded,
  type Tag,
  type Task,
  type View,
  emptyWorkspace,
  laneLayout,
  parseDuration,
  parseQuick,
  parseQuickArgs,
  parseRange,
  parseRoundGroup,
  parseRoundMode,
  parseTime,
  parseWeekday,
  report,
  rfc3339,
  roundAndFlatten,
  unionSeconds,
} from "../src/index.ts";

interface Case {
  name: string;
  expected: unknown;
  [key: string]: unknown;
}

function load(file: string): { cases: Case[]; [key: string]: unknown } {
  const url = new URL(`../../../fixtures/${file}`, import.meta.url);
  return JSON.parse(readFileSync(fileURLToPath(url), "utf8"));
}

const ms = (value: unknown) => {
  const parsed = Date.parse(String(value));
  if (Number.isNaN(parsed)) throw new Error(`not a timestamp: ${String(value)}`);
  return parsed;
};

const roundedJson = (r: Rounded) => ({
  task: r.task,
  start: rfc3339(r.start),
  end: rfc3339(r.end),
  duration: r.duration,
  original: r.original,
  entries: r.entries,
  ...(r.day !== undefined ? { day: r.day } : {}),
});

describe("union.json", () => {
  for (const c of load("union.json").cases) {
    it(c.name, () => {
      const intervals = (c.intervals as [string, string][]).map(([s, e]) => [ms(s), ms(e)] as [number, number]);
      expect(unionSeconds(intervals)).toEqual(c.expected);
    });
  }
});

describe("round.json", () => {
  for (const c of load("round.json").cases) {
    it(c.name, () => {
      const items = (c.items as { task: string; start: string; end: string; entries: string[] }[]).map((item) => ({
        task: item.task,
        start: ms(item.start),
        end: ms(item.end),
        entries: item.entries,
      }));
      const out = roundAndFlatten(
        items,
        Number(c.grid_minutes) * 60,
        parseRoundMode(String(c.mode)),
        parseRoundGroup(String(c.group)),
        String(c.tz),
      );
      expect(out.map(roundedJson)).toEqual(c.expected);
    });
  }
});

function viewOf(raw: Record<string, unknown[]>): View {
  const workspace = emptyWorkspace();
  const times = (o: Record<string, unknown>) => ({ created: ms(o.created), updated: ms(o.updated) });
  for (const p of raw.projects as Record<string, unknown>[]) {
    workspace.projects.set(String(p.id), { ...(p as unknown as Project), ...times(p) });
  }
  for (const t of raw.tags as Record<string, unknown>[]) {
    workspace.tags.set(String(t.id), { ...(t as unknown as Tag), ...times(t) });
  }
  for (const t of raw.tasks as Record<string, unknown>[]) {
    workspace.tasks.set(String(t.id), { ...(t as unknown as Task), ...times(t) });
  }
  const entries = new Map<string, Entry>();
  for (const e of raw.entries as Record<string, unknown>[]) {
    entries.set(String(e.id), {
      ...(e as unknown as Entry),
      start: ms(e.start),
      end: e.end === null ? null : ms(e.end),
      ...times(e),
    });
  }
  return { workspace, entries };
}

describe("report.json", () => {
  const doc = load("report.json");
  const view = viewOf(doc.view as Record<string, unknown[]>);
  for (const c of doc.cases) {
    it(c.name, () => {
      const range = c.range as { from: string; to: string };
      const r = report(
        view,
        { from: ms(range.from), to: ms(range.to) },
        c.group_by as GroupBy,
        String(c.tz),
        ms(c.now),
      );
      expect({
        from: rfc3339(r.from),
        to: rfc3339(r.to),
        group_by: r.groupBy,
        groups: r.groups,
        summed: r.summed,
        wall: r.wall,
      }).toEqual(c.expected);
    });
  }
});

describe("quick.json", () => {
  for (const c of load("quick.json").cases) {
    const args = c.args as string[];
    const shape = (t: ReturnType<typeof parseQuickArgs>) => ({
      title: t.title,
      tags: t.tags,
      project: t.project ?? null,
      metadata: t.metadata,
    });
    it(`${c.name} (arguments)`, () => {
      expect(shape(parseQuickArgs(args))).toEqual(c.expected);
    });
    it(`${c.name} (one line)`, () => {
      const line = args.map((arg) => (/\s/.test(arg) ? `"${arg}"` : arg)).join(" ");
      expect(shape(parseQuick(line))).toEqual(c.expected);
    });
  }
});

describe("time.json", () => {
  for (const c of load("time.json").cases) {
    it(c.name, () => {
      const now = ms(c.now);
      const tz = String(c.tz);
      const input = String(c.input);
      let actual: unknown;
      try {
        switch (c.kind) {
          case "duration":
            actual = parseDuration(input) / 1000;
            break;
          case "time":
            actual = rfc3339(parseTime(input, now, tz));
            break;
          case "range": {
            const r = parseRange(input, now, tz, parseWeekday(String(c.week_start ?? "mon")));
            actual = { from: rfc3339(r.from), to: rfc3339(r.to) };
            break;
          }
          default:
            throw new Error(`unknown kind ${String(c.kind)}`);
        }
      } catch (error) {
        if (error instanceof Error && error.name === "DomainError") actual = "error";
        else throw error;
      }
      expect(actual).toEqual(c.expected);
    });
  }
});

describe("lanes.json", () => {
  for (const c of load("lanes.json").cases) {
    it(c.name, () => {
      const items = (c.items as { id: string; start: string; end: string | null }[]).map((item) => ({
        id: item.id,
        start: ms(item.start),
        end: item.end === null ? null : ms(item.end),
      }));
      const out = laneLayout(items, ms(c.now)).map(({ id, lane, lanes }) => ({ id, lane, lanes }));
      expect(out).toEqual(c.expected);
    });
  }
});
