import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { useActions } from "@/data/actions";
import { getLastProject, reloadLastProject } from "@/lib/device-state";
import { updateSettings } from "@/lib/settings";
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

describe("QuickCreateInput default project", () => {
  async function setup(setting = true) {
    cleanup();
    localStorage.clear();
    reloadLastProject();
    updateSettings({ stickyProject: setting });
    const store = await memoryStore();
    renderWithStore(store, <Harness />);
    const input = screen.getByLabelText("Quick create task");
    const tasks = () => [...store.getSnapshot().view.workspace.tasks.values()];
    const projectOf = (title: string) => {
      const { workspace } = store.getSnapshot().view;
      const task = tasks().find((t) => t.title === title)!;
      return task.project ? workspace.projects.get(task.project)?.name : undefined;
    };
    return { store, input, projectOf, tasks };
  }
  afterEach(() => {
    cleanup();
    updateSettings({ stickyProject: true });
  });

  it("applies the last project as a chip, and an explicit @project overrides it", async () => {
    const { input, projectOf } = await setup();
    expect(screen.queryByTestId("project-chip")).toBeNull();
    await userEvent.type(input, "Write docs @foo{Enter}");
    await waitFor(() => expect(projectOf("Write docs")).toBe("foo"));
    expect(screen.getByTestId("project-chip")).toHaveTextContent("@foo");
    await userEvent.type(input, "Fix typo{Enter}");
    await waitFor(() => expect(projectOf("Fix typo")).toBe("foo"));
    await userEvent.type(input, "Plan sprint @algo");
    expect(screen.queryByTestId("project-chip")).toBeNull();
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(projectOf("Plan sprint")).toBe("algo"));
    expect(screen.getByTestId("project-chip")).toHaveTextContent("@algo");
  });

  it("⌫ at the start drops the chip for one task only", async () => {
    const { input, projectOf } = await setup();
    await userEvent.type(input, "Seed @foo{Enter}");
    await waitFor(() => expect(projectOf("Seed")).toBe("foo"));
    await userEvent.click(input);
    await userEvent.keyboard("{Backspace}");
    expect(screen.queryByTestId("project-chip")).toBeNull();
    await userEvent.type(input, "Personal errand{Enter}");
    await waitFor(() => expect(projectOf("Personal errand")).toBeUndefined());
    expect(screen.getByTestId("project-chip")).toHaveTextContent("@foo");
  });

  it("ignores a deleted project", async () => {
    const { store, input, projectOf } = await setup();
    await userEvent.type(input, "Seed @foo{Enter}");
    await waitFor(() => expect(projectOf("Seed")).toBe("foo"));
    const id = [...store.getSnapshot().view.workspace.projects.keys()][0]!;
    localStorage.setItem("tt.lastProject", JSON.stringify("gone"));
    reloadLastProject();
    expect(id).toBeTruthy();
    await waitFor(() => expect(screen.queryByTestId("project-chip")).toBeNull());
    await userEvent.type(input, "No project{Enter}");
    await waitFor(() => expect(projectOf("No project")).toBeUndefined());
  });

  it("shows no chip when the setting is off, but still remembers the project", async () => {
    const { input, projectOf } = await setup(false);
    await userEvent.type(input, "Seed @foo{Enter}");
    await waitFor(() => expect(projectOf("Seed")).toBe("foo"));
    expect(screen.queryByTestId("project-chip")).toBeNull();
    await userEvent.type(input, "Other{Enter}");
    await waitFor(() => expect(projectOf("Other")).toBeUndefined());
    expect(getLastProject()).not.toBeNull();
  });
});
