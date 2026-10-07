// Domain operations, mirroring `tt-core/src/ops.rs`. Workspace operations run
// inside one `Automerge.change` on the workspace document (pass its root
// proxy); entry operations are pure and the caller writes the result into the
// per-year document returned by `entryYear`.

import {
  DomainError,
  type Entry,
  type Millis,
  type Project,
  type Tag,
  type Task,
  type TaskState,
  type Uuid,
  type Workspace,
  compare,
} from "./model.ts";
import {
  type Doc,
  deleteItem,
  readWorkspace,
  setTaskSeqCounter,
  writeProject,
  writeTag,
  writeTask,
} from "./schema.ts";

/** UTC calendar year whose entries document holds an entry starting at `start`. */
export function entryYear(start: Millis): number {
  return new Date(start).getUTCFullYear();
}

export function uuidV4(): Uuid {
  return crypto.randomUUID();
}

/** Time-ordered uuid (RFC 9562 v7). */
export function uuidV7(now: Millis = Date.now()): Uuid {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let ms = Math.max(0, Math.floor(now));
  for (let i = 5; i >= 0; i--) {
    bytes[i] = ms % 256;
    ms = Math.floor(ms / 256);
  }
  bytes[6] = (bytes[6]! & 0x0f) | 0x70;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export interface NewTask {
  title: string;
  description?: string;
  /** Tag names; missing tags are created. */
  tags?: string[];
  /** Project name; created when missing. */
  project?: string;
  metadata?: Record<string, string>;
  state?: TaskState;
}

export interface TaskPatch {
  title?: string;
  description?: string;
  /** Replaces the whole tag set (names). */
  tags?: string[];
  addTags?: string[];
  removeTags?: string[];
  /** `null` clears the project; a string sets it by name. */
  project?: string | null;
  /** Replaces all metadata. */
  metadata?: Record<string, string>;
  setMetadata?: Record<string, string>;
  removeMetadata?: string[];
  state?: TaskState;
}

export function nextSeq(workspace: Workspace): number {
  let max = workspace.taskSeq;
  for (const task of workspace.tasks.values()) max = Math.max(max, task.seq);
  return max + 1;
}

function cleanName(name: string, what: string): string {
  const clean = name.trim();
  if (!clean) throw new DomainError("invalid", `${what} name must not be empty`);
  return clean;
}

function findTag(workspace: Workspace, name: string): Tag | undefined {
  const tags = [...workspace.tags.values()];
  return tags.find((tag) => tag.name === name) ?? tags.find((tag) => tag.name.toLowerCase() === name.toLowerCase());
}

function findProject(workspace: Workspace, name: string): Project | undefined {
  const projects = [...workspace.projects.values()];
  return (
    projects.find((project) => project.name === name) ??
    projects.find((project) => project.name.toLowerCase() === name.toLowerCase())
  );
}

/** Returns the tag named `name`, creating it when missing. */
export function ensureTag(root: Doc, workspace: Workspace, name: string, now: Millis): Uuid {
  const clean = cleanName(name.replace(/^\++/, ""), "tag");
  const existing = findTag(workspace, clean);
  if (existing) return existing.id;
  const tag: Tag = { id: uuidV7(now), name: clean, created: now, updated: now };
  writeTag(root, tag);
  workspace.tags.set(tag.id, tag);
  return tag.id;
}

/** Returns the project named `name`, creating it when missing. */
export function ensureProject(root: Doc, workspace: Workspace, name: string, now: Millis): Uuid {
  const clean = cleanName(name.replace(/^@+/, ""), "project");
  const existing = findProject(workspace, clean);
  if (existing) return existing.id;
  const project: Project = { id: uuidV7(now), name: clean, archived: false, created: now, updated: now };
  writeProject(root, project);
  workspace.projects.set(project.id, project);
  return project.id;
}

function validateMetadata(metadata: Record<string, string>): void {
  for (const key of Object.keys(metadata)) {
    if (!key.trim()) throw new DomainError("invalid", "metadata keys must not be empty");
  }
}

export function addTask(root: Doc, input: NewTask, now: Millis): Task {
  const title = input.title.trim();
  if (!title) throw new DomainError("invalid", "task title must not be empty");
  const metadata = { ...(input.metadata ?? {}) };
  validateMetadata(metadata);
  const workspace = readWorkspace(root);
  const tags = new Set<Uuid>();
  for (const name of input.tags ?? []) tags.add(ensureTag(root, workspace, name, now));
  const project = input.project !== undefined ? ensureProject(root, workspace, input.project, now) : undefined;
  const seq = nextSeq(workspace);
  const task: Task = {
    id: uuidV7(now),
    seq,
    title,
    description: input.description ?? "",
    tags: [...tags].sort(),
    ...(project ? { project } : {}),
    metadata,
    state: input.state ?? "open",
    created: now,
    updated: now,
  };
  writeTask(root, task);
  setTaskSeqCounter(root, seq);
  return task;
}

function sameTask(a: Task, b: Task): boolean {
  return JSON.stringify(normalizeTask(a)) === JSON.stringify(normalizeTask(b));
}

function normalizeTask(task: Task): unknown {
  const metadata = Object.fromEntries(Object.entries(task.metadata).sort(([a], [b]) => compare(a, b)));
  return { ...task, tags: [...task.tags].sort(), metadata, project: task.project ?? null };
}

export function modifyTask(root: Doc, id: Uuid, patch: TaskPatch, now: Millis): Task {
  const workspace = readWorkspace(root);
  const before = workspace.tasks.get(id);
  if (!before) throw new DomainError("not-found", `task ${id}`);
  const task: Task = { ...before, tags: [...before.tags], metadata: { ...before.metadata } };
  if (patch.title !== undefined) {
    const title = patch.title.trim();
    if (!title) throw new DomainError("invalid", "task title must not be empty");
    task.title = title;
  }
  if (patch.description !== undefined) task.description = patch.description;
  const tags = new Set(task.tags);
  if (patch.tags !== undefined) {
    tags.clear();
    for (const name of patch.tags) tags.add(ensureTag(root, workspace, name, now));
  }
  for (const name of patch.addTags ?? []) tags.add(ensureTag(root, workspace, name, now));
  for (const name of patch.removeTags ?? []) {
    const tag = findTag(workspace, name.trim().replace(/^\++/, ""));
    if (tag) tags.delete(tag.id);
  }
  task.tags = [...tags].sort();
  if (patch.project === null) delete task.project;
  else if (patch.project !== undefined) task.project = ensureProject(root, workspace, patch.project, now);
  if (patch.metadata !== undefined) task.metadata = { ...patch.metadata };
  for (const [key, value] of Object.entries(patch.setMetadata ?? {})) task.metadata[key.trim()] = value;
  for (const key of patch.removeMetadata ?? []) delete task.metadata[key.trim()];
  validateMetadata(task.metadata);
  if (patch.state !== undefined) task.state = patch.state;
  if (!sameTask(task, before)) {
    task.updated = now;
    writeTask(root, task);
  }
  return task;
}

/** Writes a full task record (used to restore a previous version on undo). */
export function restoreTask(root: Doc, task: Task): void {
  writeTask(root, task);
}

export function deleteTask(root: Doc, id: Uuid): Task {
  const task = readWorkspace(root).tasks.get(id);
  if (!task) throw new DomainError("not-found", `task ${id}`);
  deleteItem(root, "tasks", id);
  return task;
}

/**
 * Tasks whose `seq` collides with an earlier-created task, with the new number
 * each should get. The earliest (by `created`, then id) keeps its seq.
 */
export function seqCollisions(workspace: Workspace): [Uuid, number][] {
  const bySeq = new Map<number, Task[]>();
  for (const task of workspace.tasks.values()) {
    const list = bySeq.get(task.seq) ?? [];
    list.push(task);
    bySeq.set(task.seq, list);
  }
  const byCreated = (a: Task, b: Task) => a.created - b.created || compare(a.id, b.id);
  const losers: Task[] = [];
  for (const tasks of bySeq.values()) {
    if (tasks.length > 1) losers.push(...tasks.sort(byCreated).slice(1));
  }
  losers.sort(byCreated);
  let next = nextSeq(workspace);
  return losers.map((task) => [task.id, next++]);
}

/** Reassigns colliding seqs and bumps the counter; returns renumbered ids. */
export function repairSeqs(root: Doc, now: Millis): Uuid[] {
  const workspace = readWorkspace(root);
  const collisions = seqCollisions(workspace);
  let max = workspace.taskSeq;
  for (const [id, seq] of collisions) {
    const task = workspace.tasks.get(id)!;
    writeTask(root, { ...task, seq, updated: now });
    max = Math.max(max, seq);
  }
  if (collisions.length) setTaskSeqCounter(root, max);
  return collisions.map(([id]) => id);
}

// ---------- entries ----------

function cleanNote(note: string | null | undefined): string | undefined {
  return note && note.trim() ? note : undefined;
}

export function newEntry(task: Uuid, start: Millis, end: Millis | null, note: string | undefined, now: Millis): Entry {
  const clean = cleanNote(note);
  // Random ids keep short (8 hex) entry prefixes unique; v7 shares time prefixes.
  return { id: uuidV4(), task, start, end, ...(clean ? { note: clean } : {}), created: now, updated: now };
}

/** Start must be strictly before end when both are set. */
export function validateEntry(entry: Entry): void {
  if (entry.end !== null && entry.end <= entry.start) {
    throw new DomainError(
      "invalid",
      `entry end ${new Date(entry.end).toISOString()} must be after start ${new Date(entry.start).toISOString()}`,
    );
  }
}

export interface EntryPatch {
  start?: Millis;
  /** `null` makes the entry running again. */
  end?: Millis | null;
  task?: Uuid;
  /** `null` clears the note. */
  note?: string | null;
}

function sameEntry(a: Entry, b: Entry): boolean {
  return a.task === b.task && a.start === b.start && a.end === b.end && (a.note ?? null) === (b.note ?? null);
}

/** Applies a patch, validating the resulting range. */
export function patchEntry(entry: Entry, patch: EntryPatch, now: Millis): Entry {
  const next: Entry = { ...entry };
  if (patch.start !== undefined) next.start = patch.start;
  if (patch.end !== undefined) next.end = patch.end;
  if (patch.task !== undefined) next.task = patch.task;
  if (patch.note !== undefined) {
    const note = cleanNote(patch.note);
    if (note) next.note = note;
    else delete next.note;
  }
  validateEntry(next);
  if (!sameEntry(next, entry)) next.updated = now;
  return next;
}

/**
 * Splits an entry at `at` into two adjacent entries on the same task. A
 * running entry's second half keeps running.
 */
export function splitEntry(entry: Entry, at: Millis, now: Millis): [Entry, Entry] {
  const upper = entry.end ?? now;
  if (at <= entry.start || at >= upper) {
    throw new DomainError("invalid", `split time ${new Date(at).toISOString()} must be strictly inside the entry`);
  }
  const first: Entry = { ...entry, end: at, updated: now };
  const second: Entry = {
    id: uuidV4(),
    task: entry.task,
    start: at,
    end: entry.end,
    ...(entry.note ? { note: entry.note } : {}),
    created: now,
    updated: now,
  };
  return [first, second];
}
