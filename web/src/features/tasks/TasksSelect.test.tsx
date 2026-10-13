import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";

import { newEntry } from "@tt/domain";

import { memoryStore, renderWithStore } from "@/test/harness";

import { TasksPage } from "./TasksPage.tsx";

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

async function setup(route = "/tasks") {
  const store = await memoryStore();
  const tasks = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"].map((title) => store.createTask({ title }));
  const entry = (task: string, h: number) => newEntry(task, Date.UTC(2026, 9, 8, h), Date.UTC(2026, 9, 8, h, 30), undefined, Date.now());
  const entries = [entry(tasks[0]!.id, 8), entry(tasks[0]!.id, 9), entry(tasks[1]!.id, 10)];
  await store.commit({ label: "seed", changes: entries.map((e) => ({ kind: "entry" as const, id: e.id, before: null, after: e })) });
  renderWithStore(
    store,
    <Routes>
      <Route path="/tasks" element={<TasksPage />} />
    </Routes>,
    route,
  );
  await screen.findAllByTestId(route.includes("board") ? "board-card" : "task-row");
  return { store, tasks };
}

const rows = () => screen.getAllByTestId("task-row");
const rowOf = (title: string) => rows().find((r) => r.textContent?.includes(title))!;
const cardOf = (title: string) => screen.getAllByTestId("board-card").find((c) => c.textContent?.includes(title))!;
const box = (row: HTMLElement) => within(row).getByRole("checkbox");
const bar = () => screen.queryByTestId("task-selection-bar");

describe("task multi-select (list)", () => {
  afterEach(cleanup);

  it("keeps the checkbox column reserved: the title does not move when a checkbox appears", async () => {
    await setup();
    const row = rowOf("Alpha");
    const before = row.style.gridTemplateColumns;
    expect(before.startsWith("16px")).toBe(true);
    const titleLeft = () => row.querySelector<HTMLElement>("[data-task-row] > span:nth-child(3)")?.className;
    const titleBefore = titleLeft();
    await userEvent.hover(row);
    expect(row.style.gridTemplateColumns).toBe(before);
    fireEvent.click(box(row));
    expect(row.style.gridTemplateColumns).toBe(before);
    expect(titleLeft()).toBe(titleBefore);
  });

  it("checkbox toggles and shift-click selects the range in list order", async () => {
    await setup();
    const list = rows();
    fireEvent.click(box(list[0]!));
    fireEvent.click(box(list[3]!), { shiftKey: true });
    expect(rows().filter((r) => r.hasAttribute("data-picked"))).toHaveLength(4);
    expect(bar()).toHaveTextContent("4 tasks · 3 entries");
    expect(bar()).toHaveTextContent("Select all 5 in view");
  });

  it("mod+A selects every task in the filter, not inside inputs; Esc clears", async () => {
    const user = userEvent.setup();
    await setup();
    await user.click(screen.getByLabelText("Search tasks"));
    await user.keyboard("{Control>}a{/Control}");
    expect(bar()).toBeNull();
    await user.keyboard("{Escape}");
    await user.keyboard("{Control>}a{/Control}");
    expect(bar()).toHaveTextContent("5 tasks");
    await user.keyboard("{Escape}");
    expect(bar()).toBeNull();
  });

  it("Space toggles the focused row and Shift+J extends", async () => {
    const user = userEvent.setup();
    await setup();
    await user.keyboard("j");
    await user.keyboard(" ");
    expect(bar()).toHaveTextContent("1 task");
    await user.keyboard("{Shift>}J{/Shift}");
    await user.keyboard("{Shift>}J{/Shift}");
    expect(bar()).toHaveTextContent("3 tasks");
  });

  it("Backspace deletes the selection (with one dialog when entries exist) and a lone focused task otherwise", async () => {
    const user = userEvent.setup();
    const { store, tasks } = await setup();
    fireEvent.click(box(rowOf("Alpha")));
    fireEvent.click(box(rowOf("Bravo")));
    fireEvent.click(box(rowOf("Echo")));
    await user.keyboard("{Backspace}");
    expect(await screen.findByText("Delete 3 tasks and 3 entries?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete 3 tasks and 3 entries" }));
    await waitFor(() => expect(store.view.entries.size).toBe(0));
    expect(store.view.workspace.tasks.has(tasks[0]!.id)).toBe(false);
    expect(store.view.workspace.tasks.has(tasks[2]!.id)).toBe(true);
    await waitFor(() => expect(bar()).toBeNull());
  });

  it("the header checkbox selects and clears everything in view", async () => {
    await setup();
    fireEvent.click(screen.getByTestId("select-all"));
    expect(bar()).toHaveTextContent("5 tasks");
    fireEvent.click(screen.getByTestId("select-all"));
    expect(bar()).toBeNull();
  });
});

describe("task multi-select (board)", () => {
  afterEach(cleanup);

  it("mod-click and Space toggle cards without navigating", async () => {
    const user = userEvent.setup();
    await setup("/tasks?view=board");
    fireEvent.click(within(cardOf("Alpha")).getByText("Alpha"), { ctrlKey: true });
    fireEvent.click(within(cardOf("Bravo")).getByText("Bravo"), { ctrlKey: true });
    expect(screen.getAllByTestId("card-badge")).toHaveLength(2);
    expect(bar()).toHaveTextContent("2 tasks · 3 entries");
    cardOf("Charlie").focus();
    await user.keyboard(" ");
    expect(screen.getAllByTestId("card-badge")).toHaveLength(3);
  });
});
