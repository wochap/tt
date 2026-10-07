// The tab's view of the user's documents: index → workspace → entries-YYYY,
// merged into one `View`, plus record-level writes that mirror tt-core (an
// entry lives in the document of its start year; moving it across years
// moves it between documents). Undo/redo is a session stack of record
// snapshots (before/after), not Automerge history.

import type { DocHandle, DocumentId, Repo } from "@automerge/automerge-repo/slim";
import {
  type Doc,
  DomainError,
  type Entry,
  type Index,
  type NewTask,
  type Task,
  type TaskPatch,
  type Uuid,
  type View,
  addTask,
  deleteItem,
  emptyWorkspace,
  entryYear,
  indexSetYear,
  initEntries,
  migrate,
  modifyTask,
  readEntries,
  readIndex,
  readWorkspace,
  repairSeqs,
  seqCollisions,
  writeEntry,
  writeTask,
} from "@tt/domain";

export type RecordChange =
  | { kind: "entry"; id: Uuid; before: Entry | null; after: Entry | null }
  | { kind: "task"; id: Uuid; before: Task | null; after: Task | null };

export interface Op {
  label: string;
  changes: RecordChange[];
}

export type LoadState = "loading" | "ready" | "error";

export interface Snapshot {
  state: LoadState;
  error?: string;
  view: View;
  /** Years whose entries documents are loaded. */
  years: number[];
  canUndo: boolean;
  canRedo: boolean;
  /** Label of the op `undo` would revert. */
  undoLabel?: string;
  redoLabel?: string;
}

interface Parsed<T> {
  doc: unknown;
  value: T;
}

const EMPTY_VIEW: View = { workspace: emptyWorkspace(), entries: new Map() };
const HISTORY_LIMIT = 200;

export class TtStore {
  readonly #repo: Repo;
  readonly #indexId: string;
  #index: DocHandle<Doc> | undefined;
  #workspace: DocHandle<Doc> | undefined;
  readonly #years = new Map<number, DocHandle<Doc>>();
  readonly #yearLoads = new Map<number, Promise<DocHandle<Doc>>>();
  readonly #parsed = new Map<string, Parsed<unknown>>();
  /** entry id → year of the document holding it. */
  #locations = new Map<Uuid, number>();
  #snapshot: Snapshot = { state: "loading", view: EMPTY_VIEW, years: [], canUndo: false, canRedo: false };
  readonly #listeners = new Set<() => void>();
  #undo: Op[] = [];
  #redo: Op[] = [];
  #rebuildQueued = false;
  #repairTimer: ReturnType<typeof setTimeout> | undefined;
  #disposed = false;
  readonly #cleanups: (() => void)[] = [];

  constructor(repo: Repo, indexId: string) {
    this.#repo = repo;
    this.#indexId = indexId;
  }

  // ---------- subscription ----------

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  getSnapshot = (): Snapshot => this.#snapshot;

  #emit(patch: Partial<Snapshot>): void {
    this.#snapshot = { ...this.#snapshot, ...patch };
    for (const listener of this.#listeners) listener();
  }

  dispose(): void {
    this.#disposed = true;
    for (const cleanup of this.#cleanups) cleanup();
    clearTimeout(this.#repairTimer);
  }

  // ---------- loading ----------

  async start(): Promise<void> {
    try {
      this.#index = await this.#find(this.#indexId);
      this.#watch(this.#index, () => void this.#syncIndex());
      await this.#syncIndex();
    } catch (error) {
      if (this.#disposed) return;
      this.#emit({
        state: "error",
        error:
          error instanceof Error && /unavailable/i.test(error.message)
            ? "Your data is not on this device yet and the server cannot be reached."
            : String(error instanceof Error ? error.message : error),
      });
    }
  }

  async #find(id: string): Promise<DocHandle<Doc>> {
    return this.#repo.find<Doc>(id as DocumentId);
  }

  #watch(handle: DocHandle<Doc>, onChange: () => void): void {
    const listener = () => onChange();
    handle.on("change", listener);
    this.#cleanups.push(() => handle.off("change", listener));
  }

  #read<T>(handle: DocHandle<Doc>, parse: (doc: Doc) => T): T {
    const doc = handle.doc();
    const cached = this.#parsed.get(handle.documentId) as Parsed<T> | undefined;
    if (cached && cached.doc === doc) return cached.value;
    const value = parse(doc);
    this.#parsed.set(handle.documentId, { doc, value });
    return value;
  }

  get index(): Index | undefined {
    return this.#index ? this.#read(this.#index, readIndex) : undefined;
  }

  async #syncIndex(): Promise<void> {
    const index = this.index;
    if (!index) return;
    if (!index.workspace) throw new DomainError("not-found", "the index document lists no workspace");
    if (!this.#workspace) {
      this.#workspace = await this.#find(index.workspace);
      this.#watch(this.#workspace, () => {
        this.#queueRebuild();
        this.#scheduleRepair();
      });
      this.#scheduleRepair();
    }
    // Current year first so the timeline renders before older years load.
    const thisYear = new Date().getUTCFullYear();
    const years = [...index.entries.keys()].sort((a, b) => Math.abs(a - thisYear) - Math.abs(b - thisYear));
    for (const year of years) await this.#loadYear(year, index.entries.get(year)!);
    this.#queueRebuild();
  }

  #loadYear(year: number, id: string): Promise<DocHandle<Doc>> {
    const existing = this.#years.get(year);
    if (existing?.documentId === id) return Promise.resolve(existing);
    const loading = this.#yearLoads.get(year);
    if (loading) return loading;
    const promise = this.#find(id).then((handle) => {
      this.#years.set(year, handle);
      this.#yearLoads.delete(year);
      this.#watch(handle, () => this.#queueRebuild());
      this.#queueRebuild();
      return handle;
    });
    this.#yearLoads.set(year, promise);
    return promise;
  }

  #queueRebuild(): void {
    if (this.#rebuildQueued) return;
    this.#rebuildQueued = true;
    queueMicrotask(() => {
      this.#rebuildQueued = false;
      if (!this.#disposed) this.#rebuild();
    });
  }

  #rebuild(): void {
    if (!this.#workspace) return;
    const workspace = this.#read(this.#workspace, readWorkspace);
    const entries = new Map<Uuid, Entry>();
    const locations = new Map<Uuid, number>();
    for (const [year, handle] of [...this.#years.entries()].sort(([a], [b]) => a - b)) {
      for (const [id, entry] of this.#read(handle, readEntries)) {
        const previous = entries.get(id);
        // During a cross-year move both documents may briefly hold the entry:
        // prefer the copy in the document of its start year, then the newest.
        const home = (e: Entry, y: number | undefined) => (entryYear(e.start) === y ? 1 : 0);
        const better =
          !previous ||
          home(entry, year) > home(previous, locations.get(id)) ||
          (home(entry, year) === home(previous, locations.get(id)) && entry.updated > previous.updated);
        if (better) {
          entries.set(id, entry);
          locations.set(id, year);
        }
      }
    }
    this.#locations = locations;
    this.#emit({
      state: "ready",
      error: undefined,
      view: { workspace, entries },
      years: [...this.#years.keys()].sort((a, b) => a - b),
    });
  }

  #scheduleRepair(): void {
    clearTimeout(this.#repairTimer);
    this.#repairTimer = setTimeout(() => {
      const handle = this.#workspace;
      if (!handle || !seqCollisions(this.#read(handle, readWorkspace)).length) return;
      handle.change((d) => void repairSeqs(d, Date.now()));
    }, 1500);
  }

  // ---------- writes ----------

  #requireWorkspace(): DocHandle<Doc> {
    if (!this.#workspace) throw new DomainError("conflict", "the workspace is still loading");
    return this.#workspace;
  }

  async #yearDoc(year: number): Promise<DocHandle<Doc>> {
    const index = this.index;
    const listed = index?.entries.get(year);
    if (listed) return this.#loadYear(year, listed);
    const pendingLoad = this.#yearLoads.get(year);
    if (pendingLoad) return pendingLoad;
    // First entry of a new year: create `entries-YYYY` and list it in the
    // index (the server accepts a new document once the index lists it).
    const handle = this.#repo.create<Doc>();
    handle.change((d) => initEntries(d, year));
    this.#years.set(year, handle);
    this.#watch(handle, () => this.#queueRebuild());
    this.#index!.change((d) => indexSetYear(d, year, handle.documentId));
    return handle;
  }

  /** Writes one side of a set of record changes. */
  async #apply(changes: RecordChange[], side: "before" | "after"): Promise<void> {
    const taskChanges = changes.filter((c) => c.kind === "task");
    if (taskChanges.length) {
      this.#requireWorkspace().change((d) => {
        migrate(d);
        for (const change of taskChanges) {
          const value = change[side] as Task | null;
          if (value) writeTask(d, value);
          else deleteItem(d, "tasks", change.id);
        }
      });
    }
    for (const change of changes) {
      if (change.kind !== "entry") continue;
      const value = change[side] as Entry | null;
      const from = this.#locations.get(change.id);
      if (value) {
        const year = entryYear(value.start);
        const target = await this.#yearDoc(year);
        target.change((d) => {
          migrate(d);
          writeEntry(d, value);
        });
        this.#locations.set(change.id, year);
        if (from !== undefined && from !== year) {
          this.#years.get(from)?.change((d) => deleteItem(d, "entries", change.id));
        }
      } else if (from !== undefined) {
        this.#years.get(from)?.change((d) => deleteItem(d, "entries", change.id));
        this.#locations.delete(change.id);
      }
    }
    this.#queueRebuild();
  }

  #history(): void {
    this.#emit({
      canUndo: this.#undo.length > 0,
      canRedo: this.#redo.length > 0,
      undoLabel: this.#undo.at(-1)?.label,
      redoLabel: this.#redo.at(-1)?.label,
    });
  }

  /** Applies an op and records it for undo. */
  async commit(op: Op): Promise<void> {
    const changes = op.changes.filter((c) => JSON.stringify(c.before) !== JSON.stringify(c.after));
    if (!changes.length) return;
    await this.#apply(changes, "after");
    this.#undo.push({ ...op, changes });
    if (this.#undo.length > HISTORY_LIMIT) this.#undo.shift();
    this.#redo = [];
    this.#history();
  }

  async undo(): Promise<Op | undefined> {
    const op = this.#undo.pop();
    if (!op) return undefined;
    await this.#apply([...op.changes].reverse(), "before");
    this.#redo.push(op);
    this.#history();
    return op;
  }

  async redo(): Promise<Op | undefined> {
    const op = this.#redo.pop();
    if (!op) return undefined;
    await this.#apply(op.changes, "after");
    this.#undo.push(op);
    this.#history();
    return op;
  }

  /** Creates a task (allocating its seq) and records the creation for undo. */
  createTask(input: NewTask, label = "Created task"): Task {
    let task: Task | undefined;
    this.#requireWorkspace().change((d) => {
      migrate(d);
      task = addTask(d, input, Date.now());
    });
    const created = task!;
    this.#undo.push({ label: `${label} #${created.seq}`, changes: [{ kind: "task", id: created.id, before: null, after: created }] });
    this.#redo = [];
    this.#history();
    this.#queueRebuild();
    return created;
  }

  /** Patches a task (tags/projects by name, created as needed). */
  updateTask(id: Uuid, patch: TaskPatch, label = "Edited task"): Task {
    let before: Task | undefined;
    let after: Task | undefined;
    this.#requireWorkspace().change((d) => {
      migrate(d);
      before = readWorkspace(d).tasks.get(id);
      after = modifyTask(d, id, patch, Date.now());
    });
    if (before && after && JSON.stringify(before) !== JSON.stringify(after)) {
      this.#undo.push({ label, changes: [{ kind: "task", id, before, after }] });
      this.#redo = [];
      this.#history();
    }
    this.#queueRebuild();
    return after!;
  }

  get view(): View {
    return this.#snapshot.view;
  }
}
