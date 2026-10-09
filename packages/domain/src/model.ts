// Domain records, mirroring `tt-core/src/model.rs`. Timestamps are integer
// milliseconds since the Unix epoch (UTC); ids are lowercase hyphenated uuids.

export type Millis = number;
export type Uuid = string;

export const TASK_STATES = ["open", "done", "archived"] as const;
export type TaskState = (typeof TASK_STATES)[number];

export function parseTaskState(value: string): TaskState {
  const state = value.trim().toLowerCase();
  if ((TASK_STATES as readonly string[]).includes(state)) return state as TaskState;
  throw new DomainError("invalid", `unknown task state ${JSON.stringify(state)} (open, done, archived)`);
}

export interface User {
  id: string;
  name: string;
}

export interface Project {
  id: Uuid;
  name: string;
  color?: string;
  archived: boolean;
  created: Millis;
  updated: Millis;
}

export interface Tag {
  id: Uuid;
  name: string;
  color?: string;
  created: Millis;
  updated: Millis;
}

export interface Task {
  id: Uuid;
  seq: number;
  title: string;
  description: string;
  /** Tag ids, sorted. */
  tags: Uuid[];
  project?: Uuid;
  metadata: Record<string, string>;
  state: TaskState;
  /** Numbers held before seq collisions renumbered the task, oldest first; absent when empty. */
  previousSeqs?: number[];
  created: Millis;
  updated: Millis;
}

/** A task renumbered away from a short id: it held `from` and now holds `to`. */
export interface RenumberedFrom {
  id: Uuid;
  title: string;
  from: number;
  to: number;
}

export interface Entry {
  id: Uuid;
  task: Uuid;
  start: Millis;
  /** `null` while running. */
  end: Millis | null;
  note?: string;
  created: Millis;
  updated: Millis;
}

export interface Workspace {
  schema: number;
  user?: User;
  projects: Map<Uuid, Project>;
  tags: Map<Uuid, Tag>;
  tasks: Map<Uuid, Task>;
  taskSeq: number;
}

export interface Index {
  schema: number;
  /** bs58check document id of the workspace document. */
  workspace?: string;
  /** UTC year → bs58check document id of `entries-YYYY`. */
  entries: Map<number, string>;
}

/** Workspace plus every loaded entry, merged across year documents. */
export interface View {
  workspace: Workspace;
  entries: Map<Uuid, Entry>;
}

export type ErrorKind = "invalid" | "not-found" | "conflict";

export class DomainError extends Error {
  readonly kind: ErrorKind;
  constructor(kind: ErrorKind, message: string) {
    super(message);
    this.kind = kind;
    this.name = "DomainError";
  }
}

export function emptyWorkspace(): Workspace {
  return { schema: 0, projects: new Map(), tags: new Map(), tasks: new Map(), taskSeq: 0 };
}

export function emptyView(): View {
  return { workspace: emptyWorkspace(), entries: new Map() };
}

export function isRunning(entry: Entry): boolean {
  return entry.end === null;
}

/** Duration in seconds; running entries count up to `now`. */
export function entryDuration(entry: Entry, now: Millis): number {
  return Math.max(0, Math.floor(((entry.end ?? now) - entry.start) / 1000));
}

/** Running entries ordered by start, then id. */
export function runningEntries(view: View): Entry[] {
  return [...view.entries.values()]
    .filter(isRunning)
    .sort((a, b) => a.start - b.start || compare(a.id, b.id));
}

export function compare(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function taskBySeq(view: View, seq: number): Task | undefined {
  for (const task of view.workspace.tasks.values()) if (task.seq === seq) return task;
  return undefined;
}

/** Tasks whose `previousSeqs` contain `seq`, ordered by their current seq. */
export function renumberedFrom(view: View, seq: number): RenumberedFrom[] {
  return [...view.workspace.tasks.values()]
    .filter((task) => task.seq !== seq && (task.previousSeqs ?? []).includes(seq))
    .map((task) => ({ id: task.id, title: task.title, from: seq, to: task.seq }))
    .sort((a, b) => a.to - b.to || compare(a.id, b.id));
}

/**
 * Resolves `#12`, `12`, a full uuid, or a unique uuid prefix (≥ 4 hex). A short
 * id resolves to the task currently holding it (see `renumberedFrom`).
 */
export function resolveTask(view: View, key: string): Task {
  const text = key.trim();
  const seqText = text.startsWith("#") ? text.slice(1) : text;
  if (/^\d+$/.test(seqText)) {
    const task = taskBySeq(view, Number(seqText));
    if (task) return task;
  }
  return resolveById(text, view.workspace.tasks, "task");
}

function resolveById<T>(key: string, items: Map<Uuid, T>, what: string): T {
  const exact = items.get(key.toLowerCase());
  if (exact) return exact;
  const prefix = key.replaceAll("-", "").toLowerCase();
  if (prefix.length < 4 || !/^[0-9a-f]+$/.test(prefix)) {
    throw new DomainError("not-found", `${what} ${key}`);
  }
  const matches = [...items.entries()].filter(([id]) => id.replaceAll("-", "").startsWith(prefix));
  if (matches.length === 1) return matches[0]![1];
  if (matches.length === 0) throw new DomainError("not-found", `${what} ${key}`);
  throw new DomainError("conflict", `${what} id prefix ${key} is ambiguous`);
}

export function tagByName(workspace: Workspace, name: string): Tag | undefined {
  const wanted = name.trim();
  const tags = [...workspace.tags.values()];
  return tags.find((tag) => tag.name === wanted) ?? tags.find((tag) => tag.name.toLowerCase() === wanted.toLowerCase());
}

export function projectByName(workspace: Workspace, name: string): Project | undefined {
  const wanted = name.trim();
  const projects = [...workspace.projects.values()];
  return (
    projects.find((project) => project.name === wanted) ??
    projects.find((project) => project.name.toLowerCase() === wanted.toLowerCase())
  );
}

/** Tag records of a task, sorted by name. */
export function taskTags(workspace: Workspace, task: Task): Tag[] {
  return task.tags
    .map((id) => workspace.tags.get(id))
    .filter((tag): tag is Tag => tag !== undefined)
    .sort((a, b) => compare(a.name, b.name));
}

export function taskProject(workspace: Workspace, task: Task): Project | undefined {
  return task.project ? workspace.projects.get(task.project) : undefined;
}
