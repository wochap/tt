import { Repo } from "@automerge/automerge-repo/slim";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { afterEach, describe, expect, it } from "vitest";

import { type Doc, initIndex, initWorkspace } from "@tt/domain";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";

import { UiProvider } from "../shell/ui-state.tsx";
import { TimelinePage } from "./TimelinePage.tsx";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function Location() {
  const location = useLocation();
  return <div data-testid="location">{location.pathname + location.search}</div>;
}

describe("empty month", () => {
  afterEach(cleanup);

  it("sits above the grid and leaves every day cell clickable", async () => {
    const repo = new Repo({ network: [] });
    const workspace = repo.create<Doc>();
    workspace.change((d) => initWorkspace(d, { id: "u", name: "mara" }));
    const index = repo.create<Doc>();
    index.change((d) => initIndex(d, workspace.documentId));
    const store = new TtStore(repo, index.documentId);
    await store.start();
    await settle();
    const session = { store, client: {} as SyncClient, auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } } };
    render(
      <MemoryRouter initialEntries={["/timeline/month?date=2026-10-01"]}>
        <SessionContext.Provider value={session}>
          <TooltipProvider>
            <UiProvider>
              <Routes>
                <Route path="/timeline/:range" element={<TimelinePage />} />
              </Routes>
              <Location />
            </UiProvider>
          </TooltipProvider>
        </SessionContext.Provider>
      </MemoryRouter>,
    );
    const card = await screen.findByTestId("empty-month");
    expect(card).toHaveTextContent("No time tracked this month");
    expect(card.className).not.toMatch(/\babsolute\b/);
    const cells = screen.getAllByTestId("month-cell");
    // In the flow before the grid, not layered over it.
    expect(card.compareDocumentPosition(cells[0]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(cells.some((cell) => card.contains(cell))).toBe(false);
    fireEvent.click(cells[10]!);
    expect(screen.getByTestId("location").textContent).toMatch(/^\/timeline\/day/);
    store.dispose();
  });
});
