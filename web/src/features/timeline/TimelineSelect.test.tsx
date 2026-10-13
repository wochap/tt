import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";

import { newEntry } from "@tt/domain";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import type { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";
import { memoryStore } from "@/test/harness";

import { UiProvider } from "../shell/ui-state.tsx";
import { TimelinePage } from "./TimelinePage.tsx";

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

const at = (h: number, m = 0) => Date.UTC(2026, 2, 10, h, m);

async function setup(route = "/timeline/day?date=2026-03-10", waitForEntries = true) {
  const store: TtStore = await memoryStore();
  const a = store.createTask({ title: "Atlas urgent" });
  const b = store.createTask({ title: "Billing work" });
  const make = (task: string, start: number, end: number) => newEntry(task, start, end, undefined, start);
  const entries = [make(a.id, at(9), at(10)), make(b.id, at(11), at(12, 30)), make(a.id, at(13), at(14)), make(b.id, at(15), at(16, 30))];
  await store.commit({ label: "seed", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
  const session = { store, client: {} as SyncClient, auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } } };
  render(
    <MemoryRouter initialEntries={[route]}>
      <SessionContext.Provider value={session}>
        <TooltipProvider>
          <UiProvider>
            <Routes>
              <Route path="/timeline/:range" element={<TimelinePage />} />
            </Routes>
          </UiProvider>
        </TooltipProvider>
      </SessionContext.Provider>
    </MemoryRouter>,
  );
  if (waitForEntries) await screen.findAllByTestId("entry-block");
  return { store, entries };
}

const blocks = () => screen.getAllByTestId("entry-block");
const press = (block: HTMLElement, init?: MouseEventInit) => fireEvent.click(within(block).getAllByRole("button")[0]!, init);
const bar = () => screen.queryByTestId("selection-bar");

describe("timeline selection", () => {
  afterEach(cleanup);

  it("V enters Select mode; clicks toggle without opening the sheet", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("v");
    expect(screen.getByRole("radio", { name: /Select/ })).toBeChecked();
    press(blocks()[0]!);
    press(blocks()[1]!);
    expect(bar()).toHaveTextContent("2 entries · 2h 30m selected");
    expect(screen.queryByTestId("entry-sheet")).toBeNull();
    expect(screen.getAllByTestId("picked-badge")).toHaveLength(2);
    press(blocks()[1]!);
    expect(bar()).toHaveTextContent("1 entry");
  });

  it("shift-click selects the start-ordered range", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("v");
    press(blocks()[0]!);
    press(blocks()[2]!, { shiftKey: true });
    expect(bar()).toHaveTextContent("3 entries");
  });

  it("Esc clears the selection, then returns to Create", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("v");
    press(blocks()[0]!);
    await user.keyboard("{Escape}");
    expect(bar()).toBeNull();
    expect(screen.getByRole("radio", { name: /Select/ })).toBeChecked();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("radio", { name: "Create" })).toBeChecked();
  });

  it("Create mode still opens the sheet on click", async () => {
    await setup();
    press(blocks()[0]!);
    expect(await screen.findByTestId("entry-sheet")).toBeInTheDocument();
    expect(bar()).toBeNull();
  });

  it("month view disables the toggle and V does nothing", async () => {
    const user = userEvent.setup();
    await setup("/timeline/month?date=2026-03-10", false);
    await screen.findByText("day & week only");
    expect(screen.getByRole("radio", { name: /Select/ })).toBeDisabled();
    expect(screen.getByText("day & week only")).toBeInTheDocument();
    await user.keyboard("v");
    expect(screen.getByRole("radio", { name: "Create" })).toBeChecked();
  });

  it("Space toggles the focused entry, J/K move focus, mod+A selects all visible", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("v");
    await user.keyboard("j");
    await user.keyboard(" ");
    expect(bar()).toHaveTextContent("1 entry");
    await user.keyboard("{Control>}a{/Control}");
    expect(bar()).toHaveTextContent("4 entries");
  });

  it("mod+A inside a text field does not select entries", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("v");
    const input = document.createElement("input");
    document.body.append(input);
    input.focus();
    await user.keyboard("{Control>}a{/Control}");
    expect(bar()).toBeNull();
    input.remove();
  });

  it("Backspace deletes the selection in one op with one toast; undo restores", async () => {
    const user = userEvent.setup();
    const { store } = await setup();
    await user.keyboard("v");
    press(blocks()[0]!);
    press(blocks()[1]!);
    const { toast } = await import("sonner");
    vi.mocked(toast).mockClear();
    await user.keyboard("{Backspace}");
    await waitFor(() => expect(store.view.entries.size).toBe(2));
    expect(vi.mocked(toast)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(toast).mock.calls[0]![0]).toBe("Deleted 2 entries");
    expect(bar()).toBeNull();
    await store.undo();
    await waitFor(() => expect(store.view.entries.size).toBe(4));
  });

  it("the bar Delete button deletes once", async () => {
    const user = userEvent.setup();
    const { store } = await setup();
    await user.keyboard("v");
    press(blocks()[2]!);
    await user.click(within(screen.getByTestId("selection-bar")).getByRole("button", { name: /Delete/ }));
    await waitFor(() => expect(store.view.entries.size).toBe(3));
  });

  it("a marquee drag selects entries and never proposes a range", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("v");
    const column = screen.getByTestId("day-column");
    fireEvent.mouseDown(column, { button: 0, clientX: 0, clientY: 0 });
    fireEvent.mouseMove(document, { clientX: 20, clientY: 20 });
    fireEvent.mouseMove(document, { clientX: 40, clientY: 40 });
    expect(await screen.findByTestId("marquee")).toBeInTheDocument();
    fireEvent.mouseUp(document, { clientX: 40, clientY: 40 });
    await waitFor(() => expect(bar()).not.toBeNull());
    expect(screen.queryByTestId("create-ghost")).toBeNull();
  });
});
