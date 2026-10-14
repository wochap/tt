import { cleanup, screen, within } from "@testing-library/react";
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
  updateSettings({ stickyProject: true, startGrid: 15, durationGrid: 15 });
});

async function renderSettings() {
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
}

describe("grid settings", () => {
  it("default to 15/15 with non-dividing starts disabled, and fall back on duration change", async () => {
    updateSettings({ startGrid: 15, durationGrid: 15 });
    await renderSettings();
    const start = await screen.findByRole("radiogroup", { name: "Start grid" });
    const duration = screen.getByRole("radiogroup", { name: "Duration grid" });
    expect(within(start).getByRole("radio", { name: "15 min" })).toBeChecked();
    expect(within(duration).getByRole("radio", { name: "15 min" })).toBeChecked();
    for (const m of ["10 min", "30 min", "60 min"]) expect(within(start).getByRole("radio", { name: m })).toBeDisabled();
    expect(within(start).getByRole("radio", { name: "5 min" })).toBeEnabled();
    expect(screen.getByText(/must divide duration/)).toBeInTheDocument();
    await userEvent.click(within(duration).getByRole("radio", { name: "10 min" }));
    expect(getSettings()).toMatchObject({ startGrid: 10, durationGrid: 10 });
    expect(within(start).getByRole("radio", { name: "10 min" })).toBeChecked();
  });
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
