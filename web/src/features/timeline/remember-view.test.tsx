import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getView, reloadViewState } from "@/lib/device-state";
import { memoryStore, renderWithStore } from "@/test/harness";

import { TasksPage } from "../tasks/TasksPage.tsx";
import { TimelinePage } from "./TimelinePage.tsx";

const page = (
  <Routes>
    <Route path="/timeline/:range" element={<TimelinePage />} />
    <Route path="/tasks" element={<TasksPage />} />
  </Routes>
);

const phoneMedia = () =>
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({ matches: true, media: query, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList,
  );

beforeEach(() => {
  localStorage.clear();
  reloadViewState();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("recording the view", () => {
  it("the timeline records range and date", async () => {
    renderWithStore(await memoryStore(), page, "/timeline/week?date=2026-10-03");
    await waitFor(() => expect(getView()).toMatchObject({ range: "week", date: "2026-10-03" }));
    expect(getView().savedOn).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("tasks records board and list", async () => {
    renderWithStore(await memoryStore(), page, "/tasks?view=board");
    await waitFor(() => expect(getView().tasksMode).toBe("board"));
    await userEvent.click(screen.getByRole("radio", { name: /List/ }));
    await waitFor(() => expect(getView().tasksMode).toBe("list"));
  });

  it("a phone layout writes nothing", async () => {
    phoneMedia();
    renderWithStore(await memoryStore(), page, "/timeline/week");
    await screen.findByTestId("phone-totals").catch(() => undefined);
    expect(getView().range).toBe("day");
    expect(localStorage.getItem("tt.view")).toBeNull();
    cleanup();
    renderWithStore(await memoryStore(), page, "/tasks?view=board");
    await new Promise((r) => setTimeout(r, 50));
    expect(localStorage.getItem("tt.view")).toBeNull();
  });
});
