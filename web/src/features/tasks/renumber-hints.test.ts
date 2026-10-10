import { describe, expect, it } from "vitest";

import type { Task } from "@tt/domain";

import { renumberHints } from "./renumber-hints.tsx";

const task = (id: string, seq: number, title: string, previousSeqs?: number[]) => ({ id, seq, title, previousSeqs }) as unknown as Task;

describe("renumberHints", () => {
  it("points from the old id to the renumbered task while another task holds it", () => {
    const tasks = [task("a", 12, "Update onboarding copy"), task("b", 17, "Rotate staging certs", [12])];
    expect(renumberHints(tasks, "#12")).toEqual([{ id: "b", title: "Rotate staging certs", from: 12, to: 17 }]);
  });

  it("works when no task holds the old id", () => {
    expect(renumberHints([task("b", 17, "Rotate staging certs", [12])], " #12 ")).toEqual([{ id: "b", title: "Rotate staging certs", from: 12, to: 17 }]);
  });

  it("lists several moved tasks by their current seq", () => {
    const tasks = [task("c", 21, "Later", [3, 12]), task("b", 17, "Earlier", [12]), task("d", 30, "Other", [4])];
    expect(renumberHints(tasks, "#12").map((h) => h.to)).toEqual([17, 21]);
  });

  it("only exact #n queries have hints", () => {
    const tasks = [task("b", 17, "Rotate staging certs", [12])];
    for (const query of ["12", "#12 certs", "rotate", "#", ""]) expect(renumberHints(tasks, query)).toEqual([]);
  });
});
