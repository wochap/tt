import { Repo } from "@automerge/automerge-repo/slim";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { afterEach, describe, expect, it } from "vitest";

import { type Doc, initIndex, initWorkspace, readWorkspace, writeTask } from "@tt/domain";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";

import { CommandPalette } from "../shell/CommandPalette.tsx";
import { UiProvider, useUi } from "../shell/ui-state.tsx";
import { TasksPage } from "./TasksPage.tsx";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** #1 "Rotate staging certs" renumbered to #17; "Update onboarding copy" holds #1 now. */
async function setup() {
  const repo = new Repo({ network: [] });
  const workspace = repo.create<Doc>();
  workspace.change((d) => initWorkspace(d, { id: "u", name: "mara" }));
  const index = repo.create<Doc>();
  index.change((d) => initIndex(d, workspace.documentId));
  const store = new TtStore(repo, index.documentId);
  await store.start();
  await settle();
  const moved = store.createTask({ title: "Rotate staging certs" });
  const holder = store.createTask({ title: "Update onboarding copy" });
  await settle();
  workspace.change((d) => {
    const tasks = readWorkspace(d).tasks;
    writeTask(d, { ...tasks.get(moved.id)!, seq: 17, previousSeqs: [1], updated: Date.now() });
    writeTask(d, { ...tasks.get(holder.id)!, seq: 1, updated: Date.now() });
  });
  await settle();
  return { store, moved, holder };
}

function Location() {
  return <div data-testid="location">{useLocation().pathname}</div>;
}

function OpenPalette({ query }: { query: string }) {
  const { openPalette } = useUi();
  return (
    <button type="button" onClick={() => openPalette("all", query)}>
      open palette
    </button>
  );
}

function renderApp(store: TtStore, path: string, ui: ReactNode) {
  const session = {
    store,
    client: {} as SyncClient,
    auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } },
  };
  return render(
    <MemoryRouter initialEntries={[path]}>
      <SessionContext.Provider value={session}>
        <TooltipProvider>
          <UiProvider>
            <Routes>
              <Route path="/tasks" element={ui} />
              <Route path="/tasks/:seq" element={null} />
            </Routes>
            <Location />
          </UiProvider>
        </TooltipProvider>
      </SessionContext.Provider>
    </MemoryRouter>,
  );
}

describe("renumber hint rows", () => {
  afterEach(cleanup);

  it("task search lists the hint under the task holding the id, and Open goes to the moved task", async () => {
    const { store } = await setup();
    renderApp(store, "/tasks?q=%231", <TasksPage />);
    const hint = await screen.findByTestId("renumber-hint");
    expect(hint).toHaveTextContent("#17 Rotate staging certs was renumbered from #1");
    const holderRow = screen.getByText("Update onboarding copy").closest("[data-task-row]")!;
    expect(holderRow.nextElementSibling).toBe(hint);
    fireEvent.click(within(hint).getByRole("button", { name: "Open #17" }));
    expect(screen.getByTestId("location")).toHaveTextContent("/tasks/17");
    store.dispose();
  });

  it("the palette skips the hint with the arrow keys and Open never starts tracking", async () => {
    const { store } = await setup();
    renderApp(
      store,
      "/tasks",
      <>
        <OpenPalette query="#1" />
        <CommandPalette />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "open palette" }));
    const hint = await screen.findByTestId("renumber-hint");
    expect(hint).toHaveTextContent("#17 Rotate staging certs was renumbered from #1");
    const options = screen.getAllByRole("option");
    expect(options.some((option) => option.contains(hint))).toBe(false);
    const holderOption = options.find((option) => option.textContent?.includes("Update onboarding copy"))!;
    expect(holderOption.nextElementSibling).toBe(hint);
    // Select the holder, then press down: the next option after the hint is selected.
    const input = screen.getByRole("combobox");
    await waitFor(() => expect(options[0]).toHaveAttribute("aria-selected", "true"));
    while (screen.getAllByRole("option").find((o) => o.getAttribute("aria-selected") === "true") !== holderOption) {
      fireEvent.keyDown(input, { key: "ArrowDown" });
    }
    fireEvent.keyDown(input, { key: "ArrowDown" });
    const next = options[options.indexOf(holderOption) + 1]!;
    expect(next).toHaveAttribute("aria-selected", "true");
    expect(hint).not.toHaveAttribute("aria-selected");
    act(() => fireEvent.click(within(hint).getByRole("button", { name: "Open #17" })));
    expect(screen.getByTestId("location")).toHaveTextContent("/tasks/17");
    expect([...store.view.entries.values()].filter((e) => e.end === null)).toHaveLength(0);
    store.dispose();
  });
});
