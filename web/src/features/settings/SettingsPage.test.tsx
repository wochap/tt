import { cleanup, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { getSettings, updateSettings } from "@/lib/settings";
import { ClientProvider } from "@/data/react";
import { SessionControlContext } from "@/data/session-control";
import type { SyncClient } from "@/sync/client";
import { memoryStore, renderWithStore } from "@/test/harness";

import { SettingsPage } from "./SettingsPage.tsx";

afterEach(() => {
  cleanup();
  updateSettings({ stickyProject: true });
});

describe("sticky project switch", () => {
  it("is on by default and its choice persists", async () => {
    localStorage.removeItem("tt.settings");
    const status = { state: "synced", pending: 0, lastSync: Date.now(), mode: "shared" };
    const client = {
      subscribe: () => () => {},
      getStatus: () => status,
    } as unknown as SyncClient;
    renderWithStore(
      await memoryStore(),
      <ClientProvider client={client}>
        <SessionControlContext.Provider value={{ logout: async () => {} }}>
          <SettingsPage />
        </SessionControlContext.Provider>
      </ClientProvider>,
    );
    const toggle = await screen.findByRole("switch", { name: "New tasks use the last project" });
    expect(toggle).toBeChecked();
    expect(screen.getByText(/Press ⌫ to drop it for one task/)).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(toggle).not.toBeChecked();
    expect(JSON.parse(localStorage.getItem("tt.settings")!).stickyProject).toBe(false);
    expect(getSettings().stickyProject).toBe(false);
  });
});
