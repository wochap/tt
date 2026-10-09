// JSON export and CSV writers, mirroring `tt-core/src/export.rs` and the
// daemon's `export` command (same inputs, same column order, same rounding).

import { compare, type Entry, entryDuration, type Millis, type Project, type Tag, type Task, taskProject, taskTags, type Uuid, type View } from "./model.ts";
import { clip } from "./intervals.ts";
import { type RoundGroup, type RoundInput, type RoundMode, type Rounded, roundAndFlatten } from "./round.ts";
import { MAX_TIME, MIN_TIME, type Range, rangeOverlaps } from "./time.ts";

export const EXPORT_FORMAT = "tt-export";
export const EXPORT_VERSION = 1;

/** RFC 3339 like chrono's serde output: `2026-10-07T12:00:00Z`, millis when present. */
export function rfc3339(at: Millis, zulu = true): string {
  const iso = new Date(at).toISOString();
  const trimmed = iso.endsWith(".000Z") ? `${iso.slice(0, -5)}Z` : iso;
  return zulu ? trimmed : `${trimmed.slice(0, -1)}+00:00`;
}

/** Entries overlapping `range`, sorted by start then id. */
export function entriesIn(view: View, range: Range, now: Millis): Entry[] {
  return [...view.entries.values()]
    .filter((entry) => rangeOverlaps(range, entry.start, entry.end ?? now))
    .sort((a, b) => a.start - b.start || compare(a.id, b.id));
}

const projectJson = (p: Project) => ({
  id: p.id,
  name: p.name,
  ...(p.color !== undefined ? { color: p.color } : {}),
  archived: p.archived,
  created: rfc3339(p.created),
  updated: rfc3339(p.updated),
});

const tagJson = (t: Tag) => ({
  id: t.id,
  name: t.name,
  ...(t.color !== undefined ? { color: t.color } : {}),
  created: rfc3339(t.created),
  updated: rfc3339(t.updated),
});

const taskJson = (t: Task) => ({
  id: t.id,
  seq: t.seq,
  title: t.title,
  description: t.description,
  tags: [...t.tags].sort(),
  ...(t.project !== undefined ? { project: t.project } : {}),
  metadata: Object.fromEntries(Object.entries(t.metadata).sort(([a], [b]) => compare(a, b))),
  state: t.state,
  ...(t.previousSeqs?.length ? { previous_seqs: t.previousSeqs } : {}),
  created: rfc3339(t.created),
  updated: rfc3339(t.updated),
});

const entryJson = (e: Entry) => ({
  id: e.id,
  task: e.task,
  start: rfc3339(e.start),
  end: e.end === null ? null : rfc3339(e.end),
  ...(e.note !== undefined ? { note: e.note } : {}),
  created: rfc3339(e.created),
  updated: rfc3339(e.updated),
});

/** The documented export (see `docs/export.md`): workspace records plus entries overlapping `range`. */
export function exportJson(view: View, range: Range | undefined, now: Millis): Record<string, unknown> {
  const entries = [...view.entries.values()]
    .filter((entry) => !range || rangeOverlaps(range, entry.start, entry.end ?? now))
    .sort((a, b) => a.start - b.start || compare(a.id, b.id));
  const byId = <T extends { id: Uuid }>(a: T, b: T) => compare(a.id, b.id);
  return {
    format: EXPORT_FORMAT,
    version: EXPORT_VERSION,
    exported_at: rfc3339(now),
    ...(range ? { range: { from: rfc3339(range.from), to: rfc3339(range.to) } } : {}),
    projects: [...view.workspace.projects.values()].sort(byId).map(projectJson),
    tags: [...view.workspace.tags.values()].sort(byId).map(tagJson),
    tasks: [...view.workspace.tasks.values()].sort((a, b) => a.seq - b.seq).map(taskJson),
    entries: entries.map(entryJson),
  };
}

const CSV_HEADER = ["id", "task_seq", "task_title", "project", "tags", "start", "end", "duration_seconds", "note", "task_id"];

function csvField(value: string): string {
  return /[",\n\r]/.test(value) ? `"${value.replaceAll('"', '""')}"` : value;
}

function csvLine(fields: string[]): string {
  return `${fields.map(csvField).join(",")}\n`;
}

function taskColumns(view: View, task: Uuid): [string, string, string, string] {
  const record = view.workspace.tasks.get(task);
  if (!record) return ["", "", "", ""];
  return [
    String(record.seq),
    record.title,
    taskProject(view.workspace, record)?.name ?? "",
    taskTags(view.workspace, record)
      .map((tag) => tag.name)
      .join(" "),
  ];
}

/** Entries as CSV with task seq, title, project, tags, start, end, duration, note. */
export function entriesCsv(view: View, entries: readonly Entry[], now: Millis): string {
  let out = csvLine(CSV_HEADER);
  for (const entry of entries) {
    const [seq, title, project, tags] = taskColumns(view, entry.task);
    out += csvLine([
      entry.id,
      seq,
      title,
      project,
      tags,
      rfc3339(entry.start, false),
      entry.end === null ? "" : rfc3339(entry.end, false),
      String(entryDuration(entry, now)),
      entry.note ?? "",
      entry.task,
    ]);
  }
  return out;
}

/** Rounded intervals as CSV; the id column lists source entry ids. */
export function roundedCsv(view: View, rounded: readonly Rounded[]): string {
  let out = csvLine(CSV_HEADER);
  for (const item of rounded) {
    const [seq, title, project, tags] = taskColumns(view, item.task);
    out += csvLine([
      item.entries.join(" "),
      seq,
      title,
      project,
      tags,
      rfc3339(item.start, false),
      rfc3339(item.end, false),
      String(item.duration),
      "",
      item.task,
    ]);
  }
  return out;
}

/** Clipped inputs for round + flatten, exactly as the daemon builds them. */
export function roundInputs(view: View, range: Range | undefined, now: Millis): RoundInput[] {
  const bounds = range ?? { from: MIN_TIME, to: MAX_TIME };
  return entriesIn(view, bounds, now).flatMap((entry) => {
    const interval = clip(entry, bounds, now);
    return interval ? [{ task: entry.task, start: interval[0], end: interval[1], entries: [entry.id] }] : [];
  });
}

export interface RoundOptions {
  gridSeconds: number;
  mode: RoundMode;
  group: RoundGroup;
}

export function roundedFor(view: View, range: Range | undefined, options: RoundOptions, tz: string, now: Millis): Rounded[] {
  return roundAndFlatten(roundInputs(view, range, now), options.gridSeconds, options.mode, options.group, tz);
}

/** The daemon's `tt-rounded` JSON document. */
export function roundedJson(view: View, range: Range | undefined, options: RoundOptions, rounded: readonly Rounded[]): Record<string, unknown> {
  return {
    format: "tt-rounded",
    version: 1,
    grid_seconds: options.gridSeconds,
    mode: options.mode,
    group: options.group,
    range: range ? { from: rfc3339(range.from), to: rfc3339(range.to) } : null,
    items: rounded.map((item) => {
      const task = view.workspace.tasks.get(item.task);
      return {
        task: task
          ? {
              ...taskJson(task),
              project: (() => {
                const project = taskProject(view.workspace, task);
                return project ? { id: project.id, name: project.name, ...(project.color ? { color: project.color } : {}) } : null;
              })(),
              tags: taskTags(view.workspace, task).map((tag) => ({ id: tag.id, name: tag.name, ...(tag.color ? { color: tag.color } : {}) })),
            }
          : null,
        task_id: item.task,
        start: rfc3339(item.start),
        end: rfc3339(item.end),
        duration: item.duration,
        original: item.original,
        entries: item.entries,
        day: item.day ?? null,
      };
    }),
    total: rounded.reduce((sum, item) => sum + item.duration, 0),
  };
}
