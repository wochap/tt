import { describe, expect, it } from "vitest";

import { emptyView, type Entry, type Task } from "@tt/domain";

import { entryMatcher } from "./select.ts";

const stamp = { created: 0, updated: 0 };
const view = emptyView();
view.workspace.projects.set("atlas", { id: "atlas", name: "Atlas", archived: false, ...stamp });
view.workspace.projects.set("billing", { id: "billing", name: "Billing", archived: false, ...stamp });
const task = (id: string, project: string | undefined, tags: string[]): Task => ({
  id, seq: 1, title: id, description: "", tags, project, metadata: {}, state: "open", ...stamp,
});
for (const t of [task("a-urgent", "atlas", ["urgent"]), task("a-plain", "atlas", []), task("b-urgent", "billing", ["urgent"]), task("loose", undefined, [])]) {
  view.workspace.tasks.set(t.id, t);
}
const entry = (taskId: string): Entry => ({ id: `e-${taskId}`, task: taskId, start: 0, end: 1, ...stamp });
const ids = (filter: { projects: string[]; tags: string[] }) =>
  ["a-urgent", "a-plain", "b-urgent", "loose", "deleted"].filter((t) => entryMatcher(view, filter)(entry(t)));

describe("entryMatcher", () => {
  it("matches everything when empty", () => {
    expect(ids({ projects: [], tags: [] })).toHaveLength(5);
  });
  it("one project", () => {
    expect(ids({ projects: ["atlas"], tags: [] })).toEqual(["a-urgent", "a-plain"]);
  });
  it("ORs within a section", () => {
    expect(ids({ projects: ["atlas", "billing"], tags: [] })).toEqual(["a-urgent", "a-plain", "b-urgent"]);
  });
  it("ANDs across sections", () => {
    expect(ids({ projects: ["atlas"], tags: ["urgent"] })).toEqual(["a-urgent"]);
  });
  it("no project includes deleted tasks", () => {
    expect(ids({ projects: ["none"], tags: [] })).toEqual(["loose", "deleted"]);
  });
  it("deleted task has no tags", () => {
    expect(ids({ projects: [], tags: ["urgent"] })).toEqual(["a-urgent", "b-urgent"]);
  });
});
