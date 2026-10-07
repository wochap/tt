import { Repo } from "@automerge/automerge-repo/slim";
import { describe, expect, it } from "vitest";

import { type Doc, initIndex, initWorkspace, newEntry } from "@tt/domain";

import { TtStore } from "./store.ts";

async function setup() {
  const repo = new Repo({ network: [] });
  const workspace = repo.create<Doc>();
  workspace.change((d) => initWorkspace(d, { id: "u", name: "mara" }));
  const index = repo.create<Doc>();
  index.change((d) => initIndex(d, workspace.documentId));
  const store = new TtStore(repo, index.documentId);
  await store.start();
  await settle();
  return { repo, store, index };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("TtStore", () => {
  it("creates tasks with seqs and writes entries into the year document of their start", async () => {
    const { store } = await setup();
    const task = store.createTask({ title: "Fix login", tags: ["backend"] });
    expect(task.seq).toBe(1);
    const a = newEntry(task.id, Date.UTC(2026, 9, 8, 9), Date.UTC(2026, 9, 8, 10), undefined, Date.now());
    const b = newEntry(task.id, Date.UTC(2027, 0, 2, 9), Date.UTC(2027, 0, 2, 10), undefined, Date.now());
    await store.commit({ label: "add", changes: [{ kind: "entry", id: a.id, before: null, after: a }, { kind: "entry", id: b.id, before: null, after: b }] });
    await settle();
    const snap = store.getSnapshot();
    expect(snap.view.entries.size).toBe(2);
    expect(snap.years).toEqual([2026, 2027]);
    expect([...store.index!.entries.keys()].sort()).toEqual([2026, 2027]);
  });

  it("moves an entry between year documents and undoes exactly", async () => {
    const { store } = await setup();
    const task = store.createTask({ title: "A" });
    const entry = newEntry(task.id, Date.UTC(2026, 11, 31, 22), Date.UTC(2026, 11, 31, 23), "note", Date.now());
    await store.commit({ label: "add", changes: [{ kind: "entry", id: entry.id, before: null, after: entry }] });
    const moved = { ...entry, start: Date.UTC(2027, 0, 1, 9), end: Date.UTC(2027, 0, 1, 10) };
    await store.commit({ label: "move", changes: [{ kind: "entry", id: entry.id, before: entry, after: moved }] });
    await settle();
    expect(store.getSnapshot().view.entries.get(entry.id)?.start).toBe(moved.start);
    expect(store.getSnapshot().view.entries.size).toBe(1);
    await store.undo();
    await settle();
    expect(store.getSnapshot().view.entries.get(entry.id)).toEqual(entry);
    await store.redo();
    await settle();
    expect(store.getSnapshot().view.entries.get(entry.id)?.start).toBe(moved.start);
  });

  it("restores a deleted entry with the same id and fields", async () => {
    const { store } = await setup();
    const task = store.createTask({ title: "A" });
    const entry = newEntry(task.id, Date.UTC(2026, 9, 8, 9), Date.UTC(2026, 9, 8, 10), "keep me", Date.now());
    await store.commit({ label: "add", changes: [{ kind: "entry", id: entry.id, before: null, after: entry }] });
    await store.commit({ label: "delete", changes: [{ kind: "entry", id: entry.id, before: entry, after: null }] });
    await settle();
    expect(store.getSnapshot().view.entries.has(entry.id)).toBe(false);
    await store.undo();
    await settle();
    expect(store.getSnapshot().view.entries.get(entry.id)).toEqual(entry);
  });

  it("records task edits for undo", async () => {
    const { store } = await setup();
    const task = store.createTask({ title: "Old" });
    store.updateTask(task.id, { title: "New", addTags: ["x"] });
    await settle();
    expect(store.getSnapshot().view.workspace.tasks.get(task.id)?.title).toBe("New");
    expect(store.getSnapshot().undoLabel).toBe("Edited task");
    await store.undo();
    await settle();
    expect(store.getSnapshot().view.workspace.tasks.get(task.id)?.title).toBe("Old");
    await store.undo(); // the creation
    await settle();
    expect(store.getSnapshot().view.workspace.tasks.has(task.id)).toBe(false);
  });
});
