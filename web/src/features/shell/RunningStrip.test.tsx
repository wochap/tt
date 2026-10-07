import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { newEntry } from "@tt/domain";

import { memoryStore, renderWithStore } from "@/test/harness";

import { RunningStrip } from "./RunningStrip.tsx";

describe("RunningStrip", () => {
  it("lists running entries and Stop all gives them the same end", async () => {
    const store = await memoryStore();
    const a = store.createTask({ title: "Fix login" });
    const b = store.createTask({ title: "On-call triage" });
    const now = Date.now();
    const e1 = newEntry(a.id, now - 3_600_000, null, undefined, now);
    const e2 = newEntry(b.id, now - 600_000, null, undefined, now);
    await store.commit({ label: "start", changes: [{ kind: "entry", id: e1.id, before: null, after: e1 }, { kind: "entry", id: e2.id, before: null, after: e2 }] });
    renderWithStore(store, <RunningStrip />);
    expect(await screen.findAllByTestId("running-chip")).toHaveLength(2);
    expect(screen.getByText("Running · 2")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Stop all/ }));
    await waitFor(() => expect(screen.queryAllByTestId("running-chip")).toHaveLength(0));
    const ends = [...store.getSnapshot().view.entries.values()].map((e) => e.end);
    expect(ends[0]).not.toBeNull();
    expect(ends[0]).toBe(ends[1]);
  });
});
