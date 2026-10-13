import { cleanup, screen } from "@testing-library/react";
import { Route, Routes, useLocation } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { formatDate } from "@tt/domain";

import { reloadViewState, updateView } from "@/lib/device-state";
import { effectiveTz, getSettings } from "@/lib/settings";
import { memoryStore, renderWithStore } from "@/test/harness";

import { todayIn } from "../timeline/geometry.ts";
import { RememberedTasks, RememberedTimeline } from "./remembered.tsx";

function Where() {
  const loc = useLocation();
  return <div data-testid="where">{loc.pathname + loc.search}</div>;
}

function Routed() {
  return (
    <Routes>
      <Route path="/timeline" element={<RememberedTimeline />} />
      <Route path="/timeline/:range" element={<Where />} />
      <Route path="/tasks" element={<RememberedTasks />} />
    </Routes>
  );
}

const today = () => formatDate(todayIn(effectiveTz(getSettings()), Date.now()));

afterEach(cleanup);

beforeEach(() => {
  localStorage.clear();
  reloadViewState();
});

describe("remembered views", () => {
  it("restores the week", async () => {
    updateView({ range: "week", date: "2020-01-01", savedOn: "2020-01-01" });
    renderWithStore(await memoryStore(), <Routed />, "/timeline");
    expect((await screen.findByTestId("where")).textContent).toBe("/timeline/week");
  });

  it("restores the date on the same day", async () => {
    updateView({ range: "day", date: "2020-01-01", savedOn: today() });
    renderWithStore(await memoryStore(), <Routed />, "/timeline");
    expect((await screen.findByTestId("where")).textContent).toBe("/timeline/day?date=2020-01-01");
  });

  it("opens on today on a new day", async () => {
    updateView({ range: "month", date: "2020-01-01", savedOn: "2020-01-01" });
    renderWithStore(await memoryStore(), <Routed />, "/timeline");
    expect((await screen.findByTestId("where")).textContent).toBe("/timeline/month");
  });

  it("an explicit view=list wins over a remembered board", async () => {
    updateView({ tasksMode: "board" });
    renderWithStore(
      await memoryStore(),
      <Routes>
        <Route path="/tasks" element={<RememberedTasks />} />
      </Routes>,
      "/tasks?view=list",
    );
    expect(await screen.findByLabelText("Quick create task")).toBeInTheDocument();
    expect(screen.queryByTestId("board-open")).toBeNull();
  });

  it("bare /tasks redirects to the board when remembered", async () => {
    updateView({ tasksMode: "board" });
    renderWithStore(
      await memoryStore(),
      <Routes>
        <Route path="/tasks" element={<RememberedTasks />} />
        <Route path="*" element={<Where />} />
      </Routes>,
      "/tasks",
    );
    // the redirect lands on /tasks?view=board, which renders the page (no loop)
    expect(await screen.findByTestId("board-open")).toBeInTheDocument();
  });
});
