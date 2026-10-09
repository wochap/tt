import { type DocHandle, Repo } from "@automerge/automerge-repo/slim";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { afterEach, describe, expect, it } from "vitest";

import { type Doc, initIndex, initWorkspace, readWorkspace, writeTask } from "@tt/domain";

import { Toaster } from "@/components/ui/sonner";
import { SessionContext } from "@/data/react";
import { type Renumbering, TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";

import { RenumberNotices } from "./RenumberToast.tsx";
import { TaskDetailPage } from "./TaskDetailPage.tsx";
import { UiProvider } from "../shell/ui-state.tsx";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

async function setup() {
  const repo = new Repo({ network: [] });
  const workspace = repo.create<Doc>();
  workspace.change((d) => initWorkspace(d, { id: "u", name: "mara" }));
  const index = repo.create<Doc>();
  index.change((d) => initIndex(d, workspace.documentId));
  const store = new TtStore(repo, index.documentId);
  await store.start();
  await settle();
  return { store, workspace };
}

/** Moves `id` to `to` the way a merged seq repair does. */
function renumber(workspace: DocHandle<Doc>, moves: [string, number][]) {
  workspace.change((d) => {
    for (const [id, to] of moves) {
      const task = readWorkspace(d).tasks.get(id)!;
      writeTask(d, { ...task, seq: to, previousSeqs: [...(task.previousSeqs ?? []), task.seq], updated: Date.now() });
    }
  });
}

function Location() {
  return <div data-testid="location">{useLocation().pathname}</div>;
}

function renderApp(store: TtStore) {
  const session = {
    store,
    client: {} as SyncClient,
    auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } },
  };
  return render(
    <MemoryRouter initialEntries={["/tasks"]}>
      <SessionContext.Provider value={session}>
        <UiProvider>
          <Routes>
            <Route path="/tasks" element={null} />
            <Route path="/tasks/:seq" element={<TaskDetailPage />} />
          </Routes>
          <Location />
          <Toaster />
          <RenumberNotices />
        </UiProvider>
      </SessionContext.Provider>
    </MemoryRouter>,
  );
}

// fireEvent rather than userEvent: sonner swipe handling needs pointer capture, which jsdom lacks.
describe("renumber notice", () => {
  afterEach(cleanup);

  it("reports a local seq repair through onRenumber", async () => {
    const { store, workspace } = await setup();
    const a = store.createTask({ title: "Kept" });
    const b = store.createTask({ title: "Fix login" });
    await settle();
    const seen: Renumbering[][] = [];
    store.onRenumber((changes) => seen.push(changes));
    // A merged collision: both tasks hold #1; the store repairs it itself.
    workspace.change((d) => writeTask(d, { ...readWorkspace(d).tasks.get(b.id)!, seq: a.seq, created: a.created + 1 }));
    await waitFor(() => expect(seen).toHaveLength(1), { timeout: 4000 });
    expect(seen[0]).toEqual([{ id: b.id, from: 1, to: 3, title: "Fix login" }]);
    store.dispose();
  });

  it("shows one renumbering with a View action that opens the task", async () => {
    const { store, workspace } = await setup();
    const task = store.createTask({ title: "Fix login" });
    await settle();
    renderApp(store);
    act(() => renumber(workspace, [[task.id, 31]]));
    const toast = await screen.findByText(/is now/);
    expect(toast.closest("[data-sonner-toast]")).toHaveTextContent("Task #1 is now #31 · Fix login");
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    expect(screen.getByTestId("location")).toHaveTextContent("/tasks/31");
    expect(await screen.findByText("previously")).toHaveTextContent("previously #1");
    store.dispose();
  });

  it("collapses three renumberings into one toast listing each", async () => {
    const { store, workspace } = await setup();
    const tasks = ["Fix login redirect loop", "Billing CSV export", "Rotate staging certs"].map((title) => store.createTask({ title }));
    await settle();
    renderApp(store);
    act(() => renumber(workspace, tasks.map((t, i) => [t.id, 15 + i])));
    const toggle = await screen.findByRole("button", { name: /3 tasks renumbered/ });
    expect(document.querySelectorAll("[data-sonner-toast]")).toHaveLength(1);
    expect(screen.queryByRole("list", { name: "Renumbered tasks" })).toBeNull();
    fireEvent.click(toggle);
    const rows = within(screen.getByRole("list", { name: "Renumbered tasks" })).getAllByRole("listitem");
    expect(rows.map((row) => row.textContent)).toEqual([
      "#1→#15Fix login redirect loopView",
      "#2→#16Billing CSV exportView",
      "#3→#17Rotate staging certsView",
    ]);
    fireEvent.click(screen.getAllByRole("button", { name: "View" })[1]!);
    expect(screen.getByTestId("location")).toHaveTextContent("/tasks/16");
    store.dispose();
  });
});
