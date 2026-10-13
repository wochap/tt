import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { newEntry } from "@tt/domain";

import type { TtStore } from "@/data/store";
import { memoryStore, renderWithStore } from "@/test/harness";

import { Board } from "./Board.tsx";
import { TaskMenu, useDeleteTask } from "./delete-task.tsx";
import { TaskDetailPage } from "./TaskDetailPage.tsx";

const toasts = vi.hoisted(() => ({ calls: [] as string[] }));
vi.mock("sonner", () => ({
  toast: Object.assign((message: string) => void toasts.calls.push(message), { error: (m: string) => void toasts.calls.push(`error: ${m}`) }),
}));

async function withEntries(count: number) {
  const store = await memoryStore();
  const task = store.createTask({ title: "Fix login" });
  const entries = Array.from({ length: count }, (_, i) =>
    newEntry(task.id, Date.UTC(2026, 9, 8, 9 + i), Date.UTC(2026, 9, 8, 9 + i, 30), undefined, Date.now()),
  );
  if (entries.length) await store.commit({ label: "add", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
  return { store, task };
}

function renderDetail(store: TtStore, seq: number) {
  return renderWithStore(
    store,
    <Routes>
      <Route path="/tasks/:seq" element={<TaskDetailPage />} />
      <Route path="/tasks" element={<div data-testid="task-list" />} />
    </Routes>,
    `/tasks/${seq}`,
  );
}

describe("delete task", () => {
  afterEach(cleanup);
  beforeEach(() => {
    document.body.style.pointerEvents = "";
  });

  it("detail page: an unused task is deleted at once with an undo toast", async () => {
    toasts.calls.length = 0;
    const { store, task } = await withEntries(0);
    renderDetail(store, task.seq);
    await userEvent.click(screen.getByTestId("task-delete"));
    await waitFor(() => expect(store.view.workspace.tasks.has(task.id)).toBe(false));
    expect(toasts.calls).toContain("Deleted #1 Fix login");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await screen.findByTestId("task-list")).toBeInTheDocument();
  });

  it("asks first when entries exist; Cancel and Escape change nothing; confirm deletes with the entry-count toast", async () => {
    toasts.calls.length = 0;
    const { store, task } = await withEntries(3);
    renderDetail(store, task.seq);
    await userEvent.click(screen.getByTestId("task-delete"));
    expect(await screen.findByText("This task has 3 time entries. They will be deleted too.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(store.view.workspace.tasks.has(task.id)).toBe(true);
    await userEvent.click(screen.getByTestId("task-delete"));
    await screen.findByRole("dialog");
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(store.view.entries.size).toBe(3);

    await userEvent.click(screen.getByTestId("task-delete"));
    await userEvent.click(await screen.findByRole("button", { name: "Delete task and 3 entries" }));
    await waitFor(() => expect(store.view.workspace.tasks.has(task.id)).toBe(false));
    expect(store.view.entries.size).toBe(0);
    expect(toasts.calls).toContain("Deleted #1 and 3 entries");
  });

  it("the Backspace key starts the flow on the open task", async () => {
    const { store, task } = await withEntries(1);
    renderDetail(store, task.seq);
    await userEvent.keyboard("{Backspace}");
    expect(await screen.findByRole("button", { name: "Delete task and 1 entry" })).toBeInTheDocument();
  });

  it("the menu trigger keeps pointerdown from a draggable parent", async () => {
    const { store, task } = await withEntries(0);
    const reached = vi.fn();
    renderWithStore(
      store,
      <div onPointerDown={reached}>
        <TaskMenu task={store.view.workspace.tasks.get(task.id)!} onDelete={() => undefined} stopPointer />
      </div>,
    );
    fireEvent.pointerDown(screen.getByTestId("task-menu"), { button: 0, pointerId: 1 });
    expect(reached).not.toHaveBeenCalled();
  });

  it("board card: the overflow menu opens, Delete starts the flow, and nothing is dragged", async () => {
    const { store, task } = await withEntries(0);
    const hit = { task: store.view.workspace.tasks.get(task.id)!, matches: [], score: 0 };
    const onDelete = vi.fn();
    function Parent() {
      const [menuFor, setMenuFor] = useState<string>();
      return <Board hits={[hit as never]} totals={new Map()} setFocus={() => undefined} onDelete={onDelete} menuFor={menuFor} setMenuFor={setMenuFor} />;
    }
    renderWithStore(store, <Parent />);
    const trigger = screen.getByTestId("task-menu");
    await userEvent.click(trigger);
    expect(await screen.findByTestId("task-menu-delete")).toBeInTheDocument();
    await userEvent.click(screen.getByTestId("task-menu-delete"));
    expect(onDelete).toHaveBeenCalledWith(expect.objectContaining({ id: task.id }));
    expect(store.view.workspace.tasks.get(task.id)?.state).toBe("open");
  });
});

describe("bulk delete tasks", () => {
  afterEach(cleanup);
  beforeEach(() => {
    document.body.style.pointerEvents = "";
  });

  async function three() {
    const store = await memoryStore();
    const tasks = ["One", "Two", "Three"].map((title) => store.createTask({ title }));
    const entry = (task: string, h: number) => newEntry(task, Date.UTC(2026, 9, 8, h), Date.UTC(2026, 9, 8, h, 30), undefined, Date.now());
    const entries = [entry(tasks[0]!.id, 9), entry(tasks[0]!.id, 10), entry(tasks[1]!.id, 11)];
    await store.commit({ label: "add", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
    return { store, tasks };
  }

  function Harness({ store, ids }: { store: TtStore; ids: string[] }) {
    const del = useDeleteTask();
    return (
      <>
        <button type="button" onClick={() => del.request(ids.map((id) => store.view.workspace.tasks.get(id as never)!))}>
          go
        </button>
        {del.dialog}
      </>
    );
  }

  it("tasks without entries are deleted at once, no dialog", async () => {
    toasts.calls.length = 0;
    const { store, tasks } = await three();
    renderWithStore(store, <Harness store={store} ids={[tasks[2]!.id]} />);
    await userEvent.click(screen.getByText("go"));
    await waitFor(() => expect(store.view.workspace.tasks.has(tasks[2]!.id)).toBe(false));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("one dialog lists tasks with counts; Cancel/Esc change nothing; confirm deletes with force", async () => {
    toasts.calls.length = 0;
    const { store, tasks } = await three();
    renderWithStore(store, <Harness store={store} ids={tasks.map((t) => t.id)} />);
    await userEvent.click(screen.getByText("go"));
    expect(await screen.findByText("Delete 3 tasks and 3 entries?")).toBeInTheDocument();
    const list = screen.getByTestId("bulk-delete-list");
    expect(list).toHaveTextContent("One");
    expect(list).toHaveTextContent("2 entries");
    expect(list).toHaveTextContent("1 entry");
    expect(list).not.toHaveTextContent("Three");
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(store.view.entries.size).toBe(3);
    await userEvent.click(screen.getByText("go"));
    await screen.findByRole("dialog");
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(store.view.entries.size).toBe(3);

    await userEvent.click(screen.getByText("go"));
    await userEvent.click(await screen.findByRole("button", { name: "Delete 3 tasks and 3 entries" }));
    await waitFor(() => expect(store.view.entries.size).toBe(0));
    expect(toasts.calls).toContain("Deleted 3 tasks and 3 entries");
  });
});
