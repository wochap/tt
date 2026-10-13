import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router";
import { afterEach, describe, expect, it } from "vitest";

import { newEntry } from "@tt/domain";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import type { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";
import { memoryStore } from "@/test/harness";

import { UiProvider } from "../shell/ui-state.tsx";
import { monthCells } from "./MonthView.tsx";
import { TimelinePage } from "./TimelinePage.tsx";

const DAY = "2026-03-10";
const at = (h: number, m = 0) => Date.UTC(2026, 2, 10, h, m);

function Probe() {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <div data-testid="location">{location.pathname + location.search}</div>
      <button type="button" onClick={() => void navigate(-1)}>
        history-back
      </button>
    </>
  );
}

async function setup(route: (ids: { atlas: string; billing: string; urgent: string }) => string) {
  const store: TtStore = await memoryStore();
  const a = store.createTask({ title: "Atlas urgent", project: "Atlas", tags: ["urgent"] });
  const b = store.createTask({ title: "Billing work", project: "Billing" });
  const c = store.createTask({ title: "Loose end" });
  const make = (task: string, start: number, end: number) => newEntry(task, start, end, undefined, start);
  const entries = [make(a.id, at(9), at(10)), make(b.id, at(11), at(12, 30)), make(c.id, at(13), at(14)), make(a.id, at(15), at(16, 30))];
  await store.commit({ label: "seed", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
  const { workspace } = store.getSnapshot().view;
  const ids = {
    atlas: [...workspace.projects.values()].find((p) => p.name === "Atlas")!.id,
    billing: [...workspace.projects.values()].find((p) => p.name === "Billing")!.id,
    urgent: [...workspace.tags.values()].find((t) => t.name === "urgent")!.id,
  };
  const session = { store, client: {} as SyncClient, auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } } };
  render(
    <MemoryRouter initialEntries={[route(ids)]}>
      <SessionContext.Provider value={session}>
        <TooltipProvider>
          <UiProvider>
            <Probe />
            <Routes>
              <Route path="/timeline/:range" element={<TimelinePage />} />
            </Routes>
          </UiProvider>
        </TooltipProvider>
      </SessionContext.Provider>
    </MemoryRouter>,
  );
  await screen.findAllByTestId("entry-block");
  return { store, ids };
}

const blocks = () => screen.getAllByTestId("entry-block");
const dimmed = () => blocks().filter((b) => b.hasAttribute("data-dimmed"));
const location = () => screen.getByTestId("location").textContent;

describe("timeline filter", () => {
  afterEach(cleanup);

  it("dims non-matching entries in place, keeps lanes, and makes them inert", async () => {
    await setup((ids) => `/timeline/day?date=${DAY}&project=${ids.atlas}`);
    expect(blocks()).toHaveLength(4);
    expect(dimmed()).toHaveLength(2);
    for (const block of dimmed()) {
      expect(block.getAttribute("data-lanes")).toBe("1");
      expect(within(block).getByRole("button")).toHaveAttribute("aria-disabled", "true");
      expect(within(block).getByRole("button")).toHaveAttribute("tabindex", "-1");
      expect(within(block).queryAllByLabelText(/Resize|Drag to stop/)).toHaveLength(0);
      expect(block.style.pointerEvents).toBe("none");
      expect(block.style.opacity).toBe("0.32");
    }
    const chip = screen.getByTestId("filter-chip");
    expect(chip).toHaveTextContent("project");
    expect(chip).toHaveTextContent("Atlas");
  });

  it("shows filtered totals next to the full figure", async () => {
    await setup((ids) => `/timeline/day?date=${DAY}&project=${ids.atlas}`);
    // 2h30m on Atlas of 5h tracked.
    expect(screen.getByTestId("totals-filtered")).toHaveTextContent("Filtered · 2h 30m of 5h");
    expect(screen.getByTestId("total-summed")).toHaveTextContent("2h 30m of 5h");
  });

  it("ANDs project and tag; ignores unknown ids", async () => {
    await setup((ids) => `/timeline/day?date=${DAY}&project=${ids.billing},gone&tag=${ids.urgent}`);
    expect(dimmed()).toHaveLength(4);
    expect(screen.getAllByTestId("filter-chip")).toHaveLength(2);
    cleanup();
    await setup(() => `/timeline/day?date=${DAY}&project=gone`);
    expect(dimmed()).toHaveLength(0);
    expect(screen.queryByTestId("filter-chips")).toBeNull();
  });

  it("J/K focus skips dimmed entries", async () => {
    await setup((ids) => `/timeline/day?date=${DAY}&project=${ids.atlas}`);
    fireEvent.keyDown(window, { key: "j" });
    const first = document.activeElement?.closest("[data-entry-id]");
    expect(first?.hasAttribute("data-dimmed")).toBe(false);
    fireEvent.keyDown(window, { key: "j" });
    const second = document.activeElement?.closest("[data-entry-id]");
    expect(second).not.toBe(first);
    expect(second?.hasAttribute("data-dimmed")).toBe(false);
  });

  it("F opens the popover; typing then Enter selects and closes; ⇧F clears; Back restores", async () => {
    const user = userEvent.setup();
    await setup(() => `/timeline/day?date=${DAY}`);
    expect(dimmed()).toHaveLength(0);
    fireEvent.keyDown(window, { key: "f" });
    const input = await screen.findByPlaceholderText(/Filter by project or tag/);
    await waitFor(() => expect(input).toHaveFocus());
    await user.keyboard("atl{Enter}");
    await waitFor(() => expect(location()).toMatch(/project=/));
    await waitFor(() => expect(screen.queryByTestId("timeline-filter")).toBeNull());
    expect(dimmed()).toHaveLength(2);
    // Day -> week keeps the filter.
    fireEvent.keyDown(window, { key: "w" });
    await waitFor(() => expect(location()).toMatch(/\/timeline\/week.*project=/));
    fireEvent.keyDown(window, { key: "F", shiftKey: true });
    await waitFor(() => expect(location()).not.toMatch(/project=/));
    await user.click(screen.getByText("history-back"));
    await waitFor(() => expect(location()).toMatch(/\/timeline\/week.*project=/));
  });

  it("Space toggles the highlighted option and Backspace on empty search removes the last selection", async () => {
    const user = userEvent.setup();
    await setup(() => `/timeline/day?date=${DAY}`);
    fireEvent.keyDown(window, { key: "f" });
    const input = await screen.findByPlaceholderText(/Filter by project or tag/);
    await waitFor(() => expect(input).toHaveFocus());
    await user.keyboard("[Space]");
    await waitFor(() => expect(location()).toMatch(/project=|tag=/));
    expect(screen.getByTestId("timeline-filter")).toBeInTheDocument();
    await user.keyboard("{Backspace}");
    await waitFor(() => expect(location()).not.toMatch(/project=|tag=/));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("timeline-filter")).toBeNull());
  });

  it("lists tracked options with their time and finds untracked ones by search", async () => {
    const user = userEvent.setup();
    const { store } = await setup(() => `/timeline/day?date=${DAY}`);
    store.createTask({ title: "x", project: "Dormant" });
    fireEvent.keyDown(window, { key: "f" });
    const popover = await screen.findByTestId("timeline-filter");
    expect(within(popover).getByText("Atlas")).toBeInTheDocument();
    expect(within(popover).getAllByText("2h 30m")).toHaveLength(2); // Atlas and +urgent
    expect(within(popover).getByText("No project")).toBeInTheDocument();
    expect(within(popover).queryByText("Dormant")).toBeNull();
    await user.keyboard("dorm");
    expect(await within(popover).findByText("Dormant")).toBeInTheDocument();
  });

  it("renders the filter button with a count badge and a removable chip", async () => {
    const user = userEvent.setup();
    await setup((ids) => `/timeline/week?date=${DAY}&project=${ids.atlas}`);
    expect(screen.getByRole("button", { name: "Filter, 1 active" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Remove project Atlas/ }));
    await waitFor(() => expect(location()).not.toMatch(/project=/));
    expect(screen.getByRole("button", { name: "Filter" })).toBeInTheDocument();
  });
});

describe("monthCells with a filter", () => {
  it("counts only the matching entries", () => {
    const meeting = newEntry("meeting-task", at(10), at(11), undefined, 0);
    const cells = monthCells("UTC", { year: 2026, month: 3, day: 1 }, 0, at(20), [meeting]);
    const cell = cells.find((c) => c.key === DAY)!;
    expect(cell.summed).toBe(3600);
    expect(cell.top).toEqual([{ task: "meeting-task", seconds: 3600 }]);
    expect(monthCells("UTC", { year: 2026, month: 3, day: 1 }, 0, at(20), []).find((c) => c.key === DAY)!.summed).toBe(0);
  });
});
