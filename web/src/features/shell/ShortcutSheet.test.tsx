import { cleanup, screen, within } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { memoryStore, renderWithStore } from "@/test/harness";

import { ShortcutSheet } from "./ShortcutSheet.tsx";
import { useUi } from "./ui-state.tsx";

function Open() {
  const { setShortcutsOpen } = useUi();
  useEffect(() => setShortcutsOpen(true), [setShortcutsOpen]);
  return <ShortcutSheet />;
}

describe("shortcut sheet", () => {
  afterEach(cleanup);

  it("lists the selection keys and keeps C under Global", async () => {
    renderWithStore(await memoryStore(), <Open />);
    const timeline = await screen.findByRole("region", { name: "Timeline" });
    expect(within(timeline).getByText("Select / Create mode")).toBeInTheDocument();
    expect(within(timeline).getByText("Delete selection (undoable)")).toBeInTheDocument();
    expect(within(timeline).getByText("Select all visible entries")).toBeInTheDocument();
    const lists = screen.getByRole("region", { name: /Lists/ });
    expect(within(lists).getByText("Extend selection")).toBeInTheDocument();
    expect(within(lists).getByText("Select all in filter")).toBeInTheDocument();
    const global = screen.getByRole("region", { name: "Global" });
    expect(within(global).getByText("New task (quick-create syntax)")).toBeInTheDocument();
  });
});
