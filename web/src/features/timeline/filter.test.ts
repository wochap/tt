import { describe, expect, it } from "vitest";

import { emptyWorkspace } from "@tt/domain";

import { EMPTY_FILTER, NO_PROJECT, parseFilter, writeFilter } from "./filter.ts";

const workspace = emptyWorkspace();
const stamp = { created: 0, updated: 0 };
workspace.projects.set("p1", { id: "p1", name: "Atlas", archived: false, ...stamp });
workspace.tags.set("t1", { id: "t1", name: "urgent", ...stamp });
workspace.tags.set("t2", { id: "t2", name: "meeting", ...stamp });

describe("timeline filter URL", () => {
  it("round trips projects, tags and none", () => {
    const filter = { projects: ["p1", NO_PROJECT], tags: ["t1", "t2"] };
    const q = writeFilter(new URLSearchParams("date=2026-10-01"), filter);
    expect(q.toString()).toBe("date=2026-10-01&project=p1%2Cnone&tag=t1%2Ct2");
    expect(parseFilter(q, workspace)).toEqual(filter);
  });

  it("is empty without params and removes keys when cleared", () => {
    expect(parseFilter(new URLSearchParams(), workspace)).toEqual(EMPTY_FILTER);
    const q = writeFilter(new URLSearchParams("project=p1&tag=t1&date=x"), EMPTY_FILTER);
    expect(q.toString()).toBe("date=x");
  });

  it("drops unknown ids and duplicates", () => {
    const q = new URLSearchParams("project=gone,p1,p1&tag=nope");
    expect(parseFilter(q, workspace)).toEqual({ projects: ["p1"], tags: [] });
  });
});
