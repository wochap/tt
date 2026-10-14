import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { newEntry } from "@tt/domain";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import type { TtStore } from "@/data/store";
import { updateSettings } from "@/lib/settings";
import type { SyncClient } from "@/sync/client";
import { memoryStore } from "@/test/harness";

import { UiProvider } from "../shell/ui-state.tsx";
import { ReportsPage } from "./ReportsPage.tsx";

const at = (h: number, m = 0) => Date.UTC(2026, 9, 8, h, m);
const saved: Blob[] = [];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(at(18));
  updateSettings({ timeZone: "UTC", weekStart: 0, startGrid: 15, durationGrid: 15 });
  saved.length = 0;
  URL.createObjectURL = vi.fn((blob: Blob) => {
    saved.push(blob);
    return "blob:x";
  });
  URL.revokeObjectURL = vi.fn();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  updateSettings({ timeZone: "", startGrid: 15, durationGrid: 15 });
});

/** 7h 29m on Thu 8 Oct 2026, of which 3h 37m on Atlas. */
async function setup(route: (ids: { atlas: string; billing: string; billingTag: string }) => string) {
  const store: TtStore = await memoryStore();
  const a = store.createTask({ title: "Atlas work", project: "Atlas" });
  const b = store.createTask({ title: "Billing work", project: "Billing", tags: ["billing"] });
  const c = store.createTask({ title: "Loose end" });
  const make = (task: string, start: number, end: number) => newEntry(task, start, end, undefined, start);
  const entries = [make(a.id, at(9), at(10)), make(b.id, at(10), at(12)), make(a.id, at(13), at(15, 37)), make(c.id, at(15, 40), at(17, 32))];
  await store.commit({ label: "seed", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
  const { workspace } = store.getSnapshot().view;
  const ids = {
    atlas: [...workspace.projects.values()].find((p) => p.name === "Atlas")!.id,
    billing: [...workspace.projects.values()].find((p) => p.name === "Billing")!.id,
    billingTag: [...workspace.tags.values()].find((t) => t.name === "billing")!.id,
  };
  const session = { store, client: {} as SyncClient, auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } } };
  render(
    <MemoryRouter initialEntries={[route(ids)]}>
      <SessionContext.Provider value={session}>
        <TooltipProvider>
          <UiProvider>
            <Routes>
              <Route path="/reports" element={<ReportsPage />} />
            </Routes>
          </UiProvider>
        </TooltipProvider>
      </SessionContext.Provider>
    </MemoryRouter>,
  );
  await screen.findByTestId("report-totals");
  return { store, ids };
}

const lastCsv = async () => {
  await waitFor(() => expect(saved.length).toBeGreaterThan(0));
  return saved.at(-1)!.text();
};

describe("reports filter", () => {
  it("shows filtered totals and downloads only matching entries", async () => {
    await setup((ids) => `/reports?project=${ids.atlas}`);
    const totals = screen.getByTestId("report-totals");
    expect(totals).toHaveTextContent("Filtered · 3h 37m of 7h 29m");
    expect(totals).toHaveTextContent("Entries 2 of 4");
    const byTask = screen.getByRole("region", { name: "By task" });
    expect(within(byTask).getByText(/Atlas work/)).toBeInTheDocument();
    expect(within(byTask).queryByText(/Billing work/)).toBeNull();

    // Rounded CSV (default export mode).
    await userEvent.click(screen.getByTestId("download"));
    const rounded = await lastCsv();
    expect(rounded).toContain("Atlas work");
    expect(rounded).not.toContain("Billing work");
    expect(rounded).not.toContain("Loose end");

    await userEvent.click(screen.getByRole("radio", { name: "Raw CSV" }));
    await userEvent.click(screen.getByTestId("download"));
    const raw = await lastCsv();
    expect(raw).toContain("Atlas work");
    expect(raw).not.toContain("Billing work");
  });

  it("⇧F clears the filter", async () => {
    await setup((ids) => `/reports?project=${ids.billing}`);
    expect(screen.getByTestId("report-totals")).toHaveTextContent("Filtered · 2h 00m of 7h 29m");
    await userEvent.keyboard("{Shift>}F{/Shift}");
    await waitFor(() => expect(screen.getByTestId("report-totals")).toHaveTextContent("Summed 7h 29m"));
  });

  it("shows the empty state with downloads disabled when nothing matches", async () => {
    await setup((ids) => `/reports?project=${ids.atlas}&tag=${ids.billingTag}`);
    expect(await screen.findByTestId("no-match")).toHaveTextContent("No entries match these filters");
    expect(screen.getByTestId("download")).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: /Clear filters/ }));
    await waitFor(() => expect(screen.queryByTestId("no-match")).toBeNull());
    expect(screen.getByTestId("download")).toBeEnabled();
  });
});

describe("reports rounding menus", () => {
  it("start from Settings and fall back on a duration change", async () => {
    updateSettings({ startGrid: 5, durationGrid: 30 });
    await setup(() => "/reports");
    expect(screen.getByRole("button", { name: "Duration" })).toHaveTextContent("Duration 30 min");
    expect(screen.getByRole("button", { name: "Start" })).toHaveTextContent("Start 5 min");
    await userEvent.click(screen.getByRole("button", { name: "Start" }));
    await userEvent.click(screen.getByRole("menuitemradio", { name: "15 min" }));
    await userEvent.click(screen.getByRole("button", { name: "Duration" }));
    for (const name of ["10 min"]) expect(screen.getByRole("menuitemradio", { name })).not.toHaveAttribute("data-disabled");
    await userEvent.click(screen.getByRole("menuitemradio", { name: "10 min" }));
    expect(screen.getByRole("button", { name: "Start" })).toHaveTextContent("Start 10 min");
    await userEvent.click(screen.getByRole("button", { name: "Start" }));
    for (const name of [/^15 min/, /^30 min/, /^60 min/]) expect(screen.getByRole("menuitemradio", { name })).toHaveAttribute("data-disabled");
  });
});
