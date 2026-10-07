// User-level operations: each builds record changes from the current view,
// commits them as one undoable op, and shows the undo toast (bottom-left,
// 8 s, action = Undo). Destructive actions never ask; they toast.

import { useMemo } from "react";
import { toast } from "sonner";

import {
  DomainError,
  type Entry,
  type EntryPatch,
  formatClock,
  type Millis,
  type NewTask,
  newEntry,
  patchEntry,
  splitEntry,
  type Task,
  type TaskPatch,
  type TaskState,
  type Uuid,
  type View,
} from "@tt/domain";

import { getSettings, effectiveTz } from "@/lib/settings";
import { MOD } from "@/lib/utils";

import { useStore } from "./react.tsx";
import type { RecordChange, TtStore } from "./store.ts";

export function taskLabel(view: View, id: Uuid): string {
  const task = view.workspace.tasks.get(id);
  return task ? `#${task.seq} ${task.title}` : "(deleted task)";
}

function clock(at: Millis): string {
  return formatClock(effectiveTz(getSettings()), at);
}

export function reportError(error: unknown): void {
  const message = error instanceof DomainError || error instanceof Error ? error.message : String(error);
  toast.error(message);
}

export class Actions {
  readonly #store: TtStore;
  constructor(store: TtStore) {
    this.#store = store;
  }

  get view(): View {
    return this.#store.view;
  }

  #entry(id: Uuid): Entry {
    const entry = this.view.entries.get(id);
    if (!entry) throw new DomainError("not-found", `entry ${id}`);
    return entry;
  }

  #notify(message: string, undoable = true): void {
    toast(message, undoable ? { action: { label: `Undo  ${MOD}Z`, onClick: () => void this.undo() } } : {});
  }

  async #commit(label: string, changes: RecordChange[], notify: string | false = label): Promise<void> {
    try {
      await this.#store.commit({ label, changes });
      if (notify) this.#notify(notify);
    } catch (error) {
      reportError(error);
    }
  }

  // ---------- tracking ----------

  start(task: Uuid, at: Millis = Date.now(), note?: string): Entry {
    const entry = newEntry(task, at, null, note, Date.now());
    void this.#commit(`Started ${taskLabel(this.view, task)}`, [{ kind: "entry", id: entry.id, before: null, after: entry }]);
    return entry;
  }

  stop(id: Uuid, at: Millis = Date.now()): void {
    try {
      const before = this.#entry(id);
      const after = patchEntry(before, { end: Math.max(at, before.start + 1000) }, Date.now());
      void this.#commit(`Stopped ${taskLabel(this.view, before.task)}`, [{ kind: "entry", id, before, after }]);
    } catch (error) {
      reportError(error);
    }
  }

  /** Stops every running entry with one shared end time. */
  stopAll(at: Millis = Date.now()): void {
    const running = [...this.view.entries.values()].filter((e) => e.end === null);
    if (!running.length) return;
    const now = Date.now();
    const changes: RecordChange[] = running.map((before) => ({
      kind: "entry",
      id: before.id,
      before,
      after: patchEntry(before, { end: Math.max(at, before.start + 1000) }, now),
    }));
    void this.#commit(`Stopped ${running.length} running`, changes);
  }

  runningFor(task: Uuid): Entry[] {
    return [...this.view.entries.values()].filter((e) => e.task === task && e.end === null);
  }

  /** S: stop the task's running entries, or start it. */
  toggle(task: Uuid): void {
    const running = this.runningFor(task);
    if (running.length) {
      const now = Date.now();
      void this.#commit(
        `Stopped ${taskLabel(this.view, task)}`,
        running.map((before) => ({ kind: "entry", id: before.id, before, after: patchEntry(before, { end: Math.max(now, before.start + 1000) }, now) })),
      );
    } else {
      this.start(task);
    }
  }

  // ---------- entries ----------

  createEntry(task: Uuid, start: Millis, end: Millis | null, note?: string): Entry | undefined {
    try {
      const entry = newEntry(task, start, end, note, Date.now());
      patchEntry(entry, {}, Date.now()); // validates the range
      void this.#commit(
        `Added ${clock(start)} – ${end === null ? "now" : clock(end)}`,
        [{ kind: "entry", id: entry.id, before: null, after: entry }],
        `Added ${clock(start)} – ${end === null ? "now" : clock(end)} to ${taskLabel(this.view, task)}`,
      );
      return entry;
    } catch (error) {
      reportError(error);
      return undefined;
    }
  }

  /** Applies a patch; throws `DomainError` (for inline field errors) on an invalid range. */
  updateEntry(id: Uuid, patch: EntryPatch, label?: string): Entry {
    const before = this.#entry(id);
    const after = patchEntry(before, patch, Date.now());
    const text = label ?? `Edited ${taskLabel(this.view, before.task)}`;
    void this.#commit(text, [{ kind: "entry", id, before, after }]);
    return after;
  }

  moveEntry(id: Uuid, start: Millis, end: Millis | null): void {
    try {
      const before = this.#entry(id);
      this.updateEntry(id, { start, end }, `Moved ${taskLabel(this.view, before.task)} to ${clock(start)}`);
    } catch (error) {
      reportError(error);
    }
  }

  relink(id: Uuid, task: Uuid): void {
    try {
      const before = this.#entry(id);
      if (before.task === task) return;
      this.updateEntry(id, { task }, `Moved entry to ${taskLabel(this.view, task)}`);
    } catch (error) {
      reportError(error);
    }
  }

  deleteEntry(id: Uuid): void {
    const before = this.view.entries.get(id);
    if (!before) return;
    void this.#commit(`Deleted ${clock(before.start)} entry of ${taskLabel(this.view, before.task)}`, [
      { kind: "entry", id, before, after: null },
    ]);
  }

  /** A stopped entry is copied right after itself; a running one starts the task again now. */
  duplicateEntry(id: Uuid): Entry | undefined {
    const before = this.view.entries.get(id);
    if (!before) return undefined;
    const now = Date.now();
    const copy =
      before.end === null
        ? newEntry(before.task, now, null, before.note, now)
        : newEntry(before.task, before.end, before.end + (before.end - before.start), before.note, now);
    void this.#commit(`Duplicated ${taskLabel(this.view, before.task)}`, [{ kind: "entry", id: copy.id, before: null, after: copy }]);
    return copy;
  }

  split(id: Uuid, at: Millis): [Entry, Entry] | undefined {
    try {
      const before = this.#entry(id);
      const [first, second] = splitEntry(before, at, Date.now());
      void this.#commit(`Split ${taskLabel(this.view, before.task)} at ${clock(at)}`, [
        { kind: "entry", id, before, after: first },
        { kind: "entry", id: second.id, before: null, after: second },
      ]);
      return [first, second];
    } catch (error) {
      reportError(error);
      return undefined;
    }
  }

  // ---------- tasks ----------

  createTask(input: NewTask, options: { start?: boolean; quiet?: boolean } = {}): Task | undefined {
    try {
      const task = this.#store.createTask(input);
      if (options.start) {
        const entry = newEntry(task.id, Date.now(), null, undefined, Date.now());
        void this.#store.commit({ label: `Started #${task.seq}`, changes: [{ kind: "entry", id: entry.id, before: null, after: entry }] });
      }
      if (!options.quiet) this.#notify(`Created #${task.seq} ${task.title}${options.start ? " · tracking" : ""}`);
      return task;
    } catch (error) {
      reportError(error);
      return undefined;
    }
  }

  updateTask(id: Uuid, patch: TaskPatch, label?: string): Task | undefined {
    try {
      const task = this.#store.updateTask(id, patch, label ?? `Edited ${taskLabel(this.view, id)}`);
      if (label !== undefined) this.#notify(label);
      return task;
    } catch (error) {
      reportError(error);
      return undefined;
    }
  }

  setState(id: Uuid, state: TaskState): void {
    const task = this.view.workspace.tasks.get(id);
    if (!task || task.state === state) return;
    this.updateTask(id, { state }, `Marked #${task.seq} ${state}`);
  }

  // ---------- history ----------

  async undo(): Promise<void> {
    try {
      const op = await this.#store.undo();
      if (op) toast(`Undid: ${op.label}`, { action: { label: "Redo", onClick: () => void this.redo() } });
    } catch (error) {
      reportError(error);
    }
  }

  async redo(): Promise<void> {
    try {
      const op = await this.#store.redo();
      if (op) this.#notify(`Redid: ${op.label}`);
    } catch (error) {
      reportError(error);
    }
  }
}

export function useActions(): Actions {
  const store = useStore();
  return useMemo(() => new Actions(store), [store]);
}
