import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { useActions } from "@/data/actions";
import { memoryStore, renderWithStore } from "@/test/harness";

import { QuickCreateInput } from "./QuickCreateInput.tsx";

function Harness() {
  const actions = useActions();
  return <QuickCreateInput onCreate={(t) => actions.createTask(t)} onCreateAndStart={(t) => actions.createTask(t, { start: true })} />;
}

describe("QuickCreateInput", () => {
  it("parses tags, project and metadata; Ctrl+Enter creates and starts", async () => {
    const store = await memoryStore();
    renderWithStore(store, <Harness />);
    const input = screen.getByLabelText("Quick create task");
    await userEvent.type(input, "Fix login +backend +urgent @web ticket:PROJ-123");
    // live tokens are drawn in the mirror layer
    expect(screen.getByText("+backend")).toBeInTheDocument();
    await userEvent.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(store.getSnapshot().view.entries.size).toBe(1));
    const { workspace, entries } = store.getSnapshot().view;
    const task = [...workspace.tasks.values()][0]!;
    expect(task.title).toBe("Fix login");
    expect(task.tags).toHaveLength(2);
    expect(workspace.projects.get(task.project!)?.name).toBe("web");
    expect(task.metadata).toEqual({ ticket: "PROJ-123" });
    expect([...entries.values()][0]!.end).toBeNull();
    expect(input).toHaveValue("");
  });
});
