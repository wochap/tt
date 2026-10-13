import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { NewTask } from "@tt/domain";

import { reloadLastProject, setLastProject } from "@/lib/device-state";
import { memoryStore, renderWithStore } from "@/test/harness";

import { TaskPicker } from "./TaskPicker.tsx";

afterEach(cleanup);

describe("TaskPicker create row", () => {
  it("names the default project and applies it", async () => {
    localStorage.clear();
    const store = await memoryStore();
    const foo = store.createTask({ title: "Seed", project: "foo" });
    setLastProject(foo.project!);
    reloadLastProject();
    let created: NewTask | undefined;
    renderWithStore(
      store,
      <TaskPicker open anchor={new DOMRect(10, 10, 10, 10)} onPick={() => {}} onCreate={(input) => (created = input)} onClose={() => {}} />,
    );
    await userEvent.type(await screen.findByPlaceholderText(/Search tasks/), "Standup");
    expect(await screen.findByText(/Create task “Standup”/)).toHaveTextContent("@foo");
    await userEvent.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(created?.project).toBe("foo"));
  });
});
