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
