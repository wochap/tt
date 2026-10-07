// Summaries: per-group durations plus summed and wall-clock totals, mirroring
// `tt-core/src/report.rs`.

import { compare, DomainError, type Millis, type View } from "./model.ts";
import { clip, type Interval, seconds, unionSeconds } from "./intervals.ts";
import { addDays, formatDate, localDate, type Range, startOfDay } from "./time.ts";

export type GroupBy = "task" | "tag" | "project" | "day";

export function parseGroupBy(value: string): GroupBy {
  const by = value.trim().toLowerCase();
  if (by === "task" || by === "tag" || by === "project" || by === "day") return by;
  throw new DomainError("invalid", `unknown grouping ${JSON.stringify(by)} (task, tag, project, day)`);
}

export interface ReportGroup {
  /** Stable key: task/tag/project uuid, `YYYY-MM-DD`, or `none`. */
  key: string;
  label: string;
  seq?: number;
  /** Σ durations in seconds. */
  summed: number;
  /** Union of intervals in seconds. */
  wall: number;
  entries: number;
}

export interface Report {
  from: Millis;
  to: Millis;
  groupBy: GroupBy;
  groups: ReportGroup[];
  summed: number;
  wall: number;
  /** Number of entries overlapping the range. */
  count: number;
}

/** Splits an interval at local midnights. */
export function splitDays(start: Millis, end: Millis, tz: string): [day: string, start: Millis, end: Millis][] {
  const parts: [string, Millis, Millis][] = [];
  let cursor = start;
  while (cursor < end) {
    const day = localDate(tz, cursor);
    const next = Math.min(startOfDay(tz, addDays(day, 1)), end);
    parts.push([formatDate(day), cursor, next]);
    if (next <= cursor) break;
    cursor = next;
  }
  return parts;
}

/** Builds a report over every entry overlapping `range`. */
export function report(view: View, range: Range, groupBy: GroupBy, tz: string, now: Millis): Report {
  const groups = new Map<string, { label: string; seq?: number; intervals: Interval[] }>();
  const all: Interval[] = [];
  const add = (key: string, label: string, seq: number | undefined, interval: Interval) => {
    const group = groups.get(key) ?? { label, ...(seq !== undefined ? { seq } : {}), intervals: [] };
    group.intervals.push(interval);
    groups.set(key, group);
  };
  const { workspace } = view;
  for (const entry of view.entries.values()) {
    const interval = clip(entry, range, now);
    if (!interval) continue;
    all.push(interval);
    const task = workspace.tasks.get(entry.task);
    switch (groupBy) {
      case "task":
        if (task) add(task.id, task.title, task.seq, interval);
        else add("none", "(deleted task)", undefined, interval);
        break;
      case "tag": {
        const tags = task ? task.tags.map((id) => workspace.tags.get(id)).filter((tag) => tag !== undefined) : [];
        if (!tags.length) add("none", "(no tag)", undefined, interval);
        for (const tag of tags) add(tag.id, tag.name, undefined, interval);
        break;
      }
      case "project": {
        const project = task?.project ? workspace.projects.get(task.project) : undefined;
        if (project) add(project.id, project.name, undefined, interval);
        else add("none", "(no project)", undefined, interval);
        break;
      }
      case "day":
        for (const [day, start, end] of splitDays(interval[0], interval[1], tz)) add(day, day, undefined, [start, end]);
        break;
    }
  }
  const out: ReportGroup[] = [...groups.entries()]
    .sort(([a], [b]) => compare(a, b))
    .map(([key, group]) => ({
      key,
      label: group.label,
      ...(group.seq !== undefined ? { seq: group.seq } : {}),
      summed: group.intervals.reduce((sum, [s, e]) => sum + seconds(s, e), 0),
      wall: unionSeconds(group.intervals),
      entries: group.intervals.length,
    }));
  if (groupBy === "day") out.sort((a, b) => compare(a.key, b.key));
  else out.sort((a, b) => b.summed - a.summed || compare(a.label, b.label));
  return {
    from: range.from,
    to: range.to,
    groupBy,
    groups: out,
    summed: all.reduce((sum, [s, e]) => sum + seconds(s, e), 0),
    wall: unionSeconds(all),
    count: all.length,
  };
}
