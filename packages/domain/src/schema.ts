// Document schemas and typed read/write helpers, mirroring
// `tt-core/src/schema.rs`:
//
//   index      { schema, kind:"index", workspace:"<bs58>", entries:{ "2026":"<bs58>" } }
//   workspace  { schema, kind:"workspace", user:{id,name},
//                projects:{ <uuid>:{name,color?,archived,created,updated} },
//                tags:{ <uuid>:{name,color?,created,updated} },
//                tasks:{ <uuid>:{seq,title,description,tags:{<tagId>:true},project?,
//                                metadata:{k:v},state,created,updated} },
//                counters:{taskSeq} }
//   entries    { schema, kind:"entries", year, entries:{ <uuid>:{task,start,end|null,note?,created,updated} } }
//
// Strings are Automerge Text (edited in place with `updateText` so concurrent
// edits merge character-wise), timestamps integer milliseconds UTC, and tags a
// map used as a set. Readers take plain document snapshots; writers take the
// root proxy inside `Automerge.change`.

import { updateText } from "@automerge/automerge/slim";

import {
  DomainError,
  type Entry,
  type Index,
  type Millis,
  type Project,
  type Tag,
  type Task,
  type TaskState,
  type User,
  type Uuid,
  type Workspace,
  TASK_STATES,
} from "./model.ts";

export const SCHEMA_VERSION = 1;
export const KIND_INDEX = "index";
export const KIND_WORKSPACE = "workspace";
export const KIND_ENTRIES = "entries";

/** Any Automerge document root (snapshot or change proxy). */
export type Doc = Record<string, unknown>;
type Obj = Record<string, unknown>;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isUuid(value: string): boolean {
  return UUID.test(value);
}

// ---------- scalar readers ----------

function obj(value: unknown): Obj | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value) && !(value instanceof Date)
    ? (value as Obj)
    : undefined;
}

/** Text objects read as strings; scalar strings may arrive as `ImmutableString`. */
export function str(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  const o = obj(value);
  if (o && typeof o.val === "string") return o.val;
  return undefined;
}

export function int(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return Math.trunc(value);
  if (typeof value === "bigint") return Number(value);
  if (value instanceof Date) return value.getTime();
  const o = obj(value);
  if (o && typeof o.value === "number") return Math.trunc(o.value);
  return undefined;
}

function bool(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function keys(value: unknown): string[] {
  const o = obj(value);
  return o ? Object.keys(o) : [];
}

// ---------- readers ----------

export function kind(doc: Doc): string | undefined {
  return str(doc.kind);
}

export function readIndex(doc: Doc): Index {
  const entries = new Map<number, string>();
  const map = obj(doc.entries);
  for (const key of keys(map)) {
    const id = str(map?.[key]);
    if (/^-?\d+$/.test(key) && id) entries.set(Number(key), id);
  }
  return { schema: int(doc.schema) ?? 0, workspace: str(doc.workspace), entries };
}

function readTimes(o: Obj): [Millis, Millis] | undefined {
  const created = int(o.created);
  if (created === undefined) return undefined;
  return [created, int(o.updated) ?? created];
}

function readProject(o: Obj, id: Uuid): Project | undefined {
  const times = readTimes(o);
  const name = str(o.name);
  if (!times || name === undefined) return undefined;
  const color = str(o.color);
  return {
    id,
    name,
    ...(color !== undefined ? { color } : {}),
    archived: bool(o.archived) ?? false,
    created: times[0],
    updated: times[1],
  };
}

function readTag(o: Obj, id: Uuid): Tag | undefined {
  const times = readTimes(o);
  const name = str(o.name);
  if (!times || name === undefined) return undefined;
  const color = str(o.color);
  return { id, name, ...(color !== undefined ? { color } : {}), created: times[0], updated: times[1] };
}

function readState(value: unknown): TaskState {
  const state = str(value)?.trim().toLowerCase();
  return (TASK_STATES as readonly string[]).includes(state ?? "") ? (state as TaskState) : "open";
}

function readTask(o: Obj, id: Uuid): Task | undefined {
  const times = readTimes(o);
  const seq = int(o.seq);
  if (!times || seq === undefined || seq < 0) return undefined;
  const tagMap = obj(o.tags);
  const tags = keys(tagMap)
    .filter((key) => bool(tagMap?.[key]) ?? true)
    .filter(isUuid)
    .map((key) => key.toLowerCase())
    .sort();
  const metadata: Record<string, string> = {};
  const metaMap = obj(o.metadata);
  for (const key of keys(metaMap).sort()) {
    const value = str(metaMap?.[key]);
    if (value !== undefined) metadata[key] = value;
  }
  const project = str(o.project);
  return {
    id,
    seq,
    title: str(o.title) ?? "",
    description: str(o.description) ?? "",
    tags,
    ...(project && isUuid(project) ? { project: project.toLowerCase() } : {}),
    metadata,
    state: readState(o.state),
    created: times[0],
    updated: times[1],
  };
}

function readCollection<T>(doc: Doc, name: string, read: (o: Obj, id: Uuid) => T | undefined): Map<Uuid, T> {
  const out = new Map<Uuid, T>();
  const map = obj(doc[name]);
  for (const key of keys(map)) {
    const item = obj(map?.[key]);
    if (!isUuid(key) || !item) continue;
    const id = key.toLowerCase();
    const value = read(item, id);
    if (value !== undefined) out.set(id, value);
  }
  return out;
}

export function readWorkspace(doc: Doc): Workspace {
  const userObj = obj(doc.user);
  const user: User | undefined = userObj ? { id: str(userObj.id) ?? "", name: str(userObj.name) ?? "" } : undefined;
  const taskSeq = int(obj(doc.counters)?.taskSeq);
  return {
    schema: int(doc.schema) ?? 0,
    ...(user ? { user } : {}),
    projects: readCollection(doc, "projects", readProject),
    tags: readCollection(doc, "tags", readTag),
    tasks: readCollection(doc, "tasks", readTask),
    taskSeq: taskSeq !== undefined && taskSeq > 0 ? taskSeq : 0,
  };
}

export function entriesYear(doc: Doc): number | undefined {
  return int(doc.year);
}

export function readEntries(doc: Doc): Map<Uuid, Entry> {
  const out = new Map<Uuid, Entry>();
  const map = obj(doc.entries);
  for (const key of keys(map)) {
    const item = obj(map?.[key]);
    if (!isUuid(key) || !item) continue;
    const task = str(item.task);
    const start = int(item.start);
    if (!task || !isUuid(task) || start === undefined) continue;
    const created = int(item.created) ?? start;
    const note = str(item.note);
    const id = key.toLowerCase();
    out.set(id, {
      id,
      task: task.toLowerCase(),
      start,
      end: int(item.end) ?? null,
      ...(note ? { note } : {}),
      created,
      updated: int(item.updated) ?? created,
    });
  }
  return out;
}

// ---------- writers ----------

/** Writes a string as Text, editing an existing Text in place. */
export function putText(root: Doc, path: string[], value: string): void {
  const parent = resolve(root, path.slice(0, -1));
  const key = path[path.length - 1]!;
  const current = parent[key];
  if (typeof current === "string") {
    if (current !== value) updateText(root, path, value);
    return;
  }
  parent[key] = value;
}

function putOptText(root: Doc, path: string[], value: string | undefined): void {
  if (value === undefined) {
    deleteIfPresent(resolve(root, path.slice(0, -1)), path[path.length - 1]!);
  } else {
    putText(root, path, value);
  }
}

function resolve(root: Doc, path: string[]): Obj {
  let current: Obj = root;
  for (const key of path) {
    const next = obj(current[key]);
    if (!next) throw new DomainError("invalid", `document has no map at ${path.join(".")}`);
    current = next;
  }
  return current;
}

function ensureMap(parent: Obj, key: string): Obj {
  const existing = obj(parent[key]);
  if (existing) return existing;
  parent[key] = {};
  return parent[key] as Obj;
}

function deleteIfPresent(parent: Obj, key: string): void {
  if (key in parent) delete parent[key];
}

function putIntIfChanged(parent: Obj, key: string, value: number): void {
  if (int(parent[key]) !== value) parent[key] = Math.trunc(value);
}

/** Upgrades a document written with an older schema (stamps the version). */
export function migrate(root: Doc): void {
  const version = int(root.schema) ?? 0;
  if (version > SCHEMA_VERSION) {
    throw new DomainError(
      "invalid",
      `document schema ${version} is newer than supported ${SCHEMA_VERSION}; reload to update tt`,
    );
  }
  if (version < SCHEMA_VERSION) root.schema = SCHEMA_VERSION;
}

export function initIndex(root: Doc, workspace: string): void {
  root.schema = SCHEMA_VERSION;
  putText(root, ["kind"], KIND_INDEX);
  putText(root, ["workspace"], workspace);
  ensureMap(root, "entries");
}

export function indexSetYear(root: Doc, year: number, docId: string): void {
  ensureMap(root, "entries");
  putText(root, ["entries", String(year)], docId);
}

export function initWorkspace(root: Doc, user: User): void {
  root.schema = SCHEMA_VERSION;
  putText(root, ["kind"], KIND_WORKSPACE);
  ensureMap(root, "user");
  putText(root, ["user", "id"], user.id);
  putText(root, ["user", "name"], user.name);
  ensureMap(root, "projects");
  ensureMap(root, "tags");
  ensureMap(root, "tasks");
  const counters = ensureMap(root, "counters");
  if (int(counters.taskSeq) === undefined) counters.taskSeq = 0;
}

export function initEntries(root: Doc, year: number): void {
  root.schema = SCHEMA_VERSION;
  putText(root, ["kind"], KIND_ENTRIES);
  root.year = year;
  ensureMap(root, "entries");
}

function item(root: Doc, collection: string, id: Uuid): Obj {
  return ensureMap(ensureMap(root, collection), id);
}

export function writeProject(root: Doc, project: Project): void {
  const o = item(root, "projects", project.id);
  const base = ["projects", project.id];
  putText(root, [...base, "name"], project.name);
  putOptText(root, [...base, "color"], project.color);
  if (bool(o.archived) !== project.archived) o.archived = project.archived;
  putIntIfChanged(o, "created", project.created);
  putIntIfChanged(o, "updated", project.updated);
}

export function writeTag(root: Doc, tag: Tag): void {
  const o = item(root, "tags", tag.id);
  const base = ["tags", tag.id];
  putText(root, [...base, "name"], tag.name);
  putOptText(root, [...base, "color"], tag.color);
  putIntIfChanged(o, "created", tag.created);
  putIntIfChanged(o, "updated", tag.updated);
}

/** Writes only the fields that differ, so concurrent edits of other fields merge. */
export function writeTask(root: Doc, task: Task): void {
  const o = item(root, "tasks", task.id);
  const base = ["tasks", task.id];
  putIntIfChanged(o, "seq", task.seq);
  putText(root, [...base, "title"], task.title);
  putText(root, [...base, "description"], task.description);
  const tags = ensureMap(o, "tags");
  for (const key of Object.keys(tags)) {
    if (!task.tags.includes(key.toLowerCase())) delete tags[key];
  }
  for (const tag of task.tags) {
    if (tags[tag] !== true) tags[tag] = true;
  }
  putOptText(root, [...base, "project"], task.project);
  const metadata = ensureMap(o, "metadata");
  for (const key of Object.keys(metadata)) {
    if (!(key in task.metadata)) delete metadata[key];
  }
  for (const [key, value] of Object.entries(task.metadata)) {
    putText(root, [...base, "metadata", key], value);
  }
  putText(root, [...base, "state"], task.state);
  putIntIfChanged(o, "created", task.created);
  putIntIfChanged(o, "updated", task.updated);
}

export function deleteItem(root: Doc, collection: string, id: Uuid): void {
  deleteIfPresent(ensureMap(root, collection), id);
}

export function setTaskSeqCounter(root: Doc, value: number): void {
  ensureMap(root, "counters").taskSeq = value;
}

/** Writes only changed fields of an entry. */
export function writeEntry(root: Doc, entry: Entry): void {
  const o = item(root, "entries", entry.id);
  const base = ["entries", entry.id];
  putText(root, [...base, "task"], entry.task);
  putIntIfChanged(o, "start", entry.start);
  if (entry.end === null) {
    // `null` rather than absence keeps "running" explicit (matches tt-core).
    if (o.end !== null) o.end = null;
  } else {
    putIntIfChanged(o, "end", entry.end);
  }
  putOptText(root, [...base, "note"], entry.note);
  putIntIfChanged(o, "created", entry.created);
  putIntIfChanged(o, "updated", entry.updated);
}
