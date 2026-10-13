import { toast } from "sonner";
import { describe, expect, it, vi } from "vitest";

import { newEntry } from "@tt/domain";

import { memoryStore } from "../test/harness.tsx";
import { Actions } from "./actions.ts";

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

async function setup() {
  const store = await memoryStore();
  const actions = new Actions(store);
  const task = store.createTask({ title: "Fix login", tags: ["backend"] });
  const a = newEntry(task.id, Date.UTC(2026, 9, 8, 9), Date.UTC(2026, 9, 8, 10), "one", Date.now());
  const b = newEntry(task.id, Date.UTC(2027, 0, 2, 9), null, "two", Date.now());
  return { store, actions, task, a, b };
}

async function addEntries(store: Awaited<ReturnType<typeof setup>>["store"], ...entries: ReturnType<typeof newEntry>[]) {
  await store.commit({ label: "add", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
  await settle();
}

describe("Actions.deleteTask", () => {
  it("deletes a task without entries", async () => {
    const { store, actions, task } = await setup();
    expect(await actions.deleteTask(task.id)).toBe(true);
    await settle();
    expect(store.view.workspace.tasks.has(task.id)).toBe(false);
    expect(store.getSnapshot().undoLabel).toBe("Deleted #1 Fix login");
  });

  it("refuses a task with entries unless forced", async () => {
    const { store, actions, task, a } = await setup();
    await addEntries(store, a);
    expect(await actions.deleteTask(task.id)).toBe(false);
    expect(store.view.workspace.tasks.has(task.id)).toBe(true);
    expect(store.view.entries.has(a.id)).toBe(true);
  });

  it("force deletes the task and all its entries", async () => {
    const { store, actions, task, a, b } = await setup();
    await addEntries(store, a, b);
    expect(await actions.deleteTask(task.id, { force: true })).toBe(true);
    await settle();
    expect(store.view.workspace.tasks.has(task.id)).toBe(false);
    expect(store.view.entries.size).toBe(0);
  });

  it("undo restores task and entries across years in one step; redo deletes again", async () => {
    const { store, actions, task, a, b } = await setup();
    await addEntries(store, a, b);
    expect(store.getSnapshot().years).toEqual([2026, 2027]);
    await actions.deleteTask(task.id, { force: true });
    await settle();
    await store.undo();
    await settle();
    expect(store.view.workspace.tasks.get(task.id)).toEqual(task);
    expect(store.view.entries.get(a.id)).toEqual(a);
    expect(store.view.entries.get(b.id)).toEqual(b);
    await store.redo();
    await settle();
    expect(store.view.workspace.tasks.has(task.id)).toBe(false);
    expect(store.view.entries.size).toBe(0);
  });

  it("does not reuse the seq of a deleted highest task", async () => {
    const { store, actions } = await setup();
    const second = store.createTask({ title: "Second" });
    expect(second.seq).toBe(2);
    await actions.deleteTask(second.id);
    await settle();
    expect(store.createTask({ title: "Third" }).seq).toBe(3);
  });
});

describe("Actions.deleteEntries", () => {
  it("deletes entries across years in one op with one toast; missing ids ignored", async () => {
    const { store, actions, a, b } = await setup();
    await addEntries(store, a, b);
    vi.mocked(toast).mockClear();
    actions.deleteEntries([a.id, b.id, "missing" as typeof a.id]);
    await settle();
    expect(store.view.entries.size).toBe(0);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(vi.mocked(toast).mock.calls[0]![0]).toBe("Deleted 2 entries");
    expect(store.getSnapshot().undoLabel).toBe("Deleted 2 entries");
  });

  it("uses singular wording and one undo restores everything; redo deletes again", async () => {
    const { store, actions, a, b } = await setup();
    await addEntries(store, a, b);
    actions.deleteEntries([a.id]);
    await settle();
    expect(vi.mocked(toast).mock.calls.at(-1)?.[0]).toBe("Deleted 1 entry");
    await store.undo();
    await settle();
    actions.deleteEntries([a.id, b.id]);
    await settle();
    await store.undo();
    await settle();
    expect(store.view.entries.get(a.id)).toEqual(a);
    expect(store.view.entries.get(b.id)).toEqual(b);
    await store.redo();
    await settle();
    expect(store.view.entries.size).toBe(0);
  });
});

describe("Actions.deleteTasks", () => {
  it("deletes unused tasks in one op", async () => {
    const { store, actions, task } = await setup();
    const other = store.createTask({ title: "Other" });
    await settle();
    const base = store.view.workspace.tasks.size - 2;
    vi.mocked(toast).mockClear();
    expect(await actions.deleteTasks([task.id, other.id])).toBe(true);
    await settle();
    expect(store.view.workspace.tasks.size).toBe(base);
    expect(vi.mocked(toast).mock.calls[0]![0]).toBe("Deleted 2 tasks");
    await store.undo();
    await settle();
    expect(store.view.workspace.tasks.get(task.id)).toEqual(task);
    expect(store.view.workspace.tasks.get(other.id)).toEqual(other);
  });

  it("refuses entries without force, deletes everything with force and undoes in one step", async () => {
    const { store, actions, task, a, b } = await setup();
    const other = store.createTask({ title: "Other" });
    await addEntries(store, a, b);
    await settle();
    const base = store.view.workspace.tasks.size - 2;
    expect(await actions.deleteTasks([task.id, other.id])).toBe(false);
    expect(store.view.workspace.tasks.size).toBe(base + 2);
    vi.mocked(toast).mockClear();
    expect(await actions.deleteTasks([task.id, other.id], { force: true })).toBe(true);
    await settle();
    expect(vi.mocked(toast).mock.calls[0]![0]).toBe("Deleted 2 tasks and 2 entries");
    expect(store.view.entries.size).toBe(0);
    await store.undo();
    await settle();
    expect(store.view.workspace.tasks.size).toBe(base + 2);
    expect(store.view.entries.get(a.id)).toEqual(a);
    expect(store.view.entries.get(b.id)).toEqual(b);
    await store.redo();
    await settle();
    expect(store.view.workspace.tasks.size).toBe(base);
  });

  it("refuses while entries are still loading", async () => {
    const { store, actions, task } = await setup();
    const snap = store.getSnapshot();
    vi.spyOn(store, "getSnapshot").mockReturnValue({ ...snap, state: "loading" } as typeof snap);
    expect(await actions.deleteTasks([task.id], { force: true })).toBe(false);
    expect(toast.error).toHaveBeenCalledWith("Still loading your entries…");
    vi.restoreAllMocks();
    expect(store.view.workspace.tasks.has(task.id)).toBe(true);
  });
});
