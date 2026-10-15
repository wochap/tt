import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ClientProvider, SessionContext } from "@/data/react";
import { SessionControlContext } from "@/data/session-control";
import type { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";
import type { Auth, SyncStatus } from "@/sync/protocol";

import { memoryStore } from "../../test/harness.tsx";
import { AppShell } from "./AppShell.tsx";

const AUTH: Auth = { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } };
const STATUS: SyncStatus = { state: "synced", pending: 0, lastSync: Date.now(), mode: "shared" };

function stubPhone(phone: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: phone && query === "(max-width: 640px)",
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

function renderShell(store: TtStore) {
  const client = { subscribe: () => () => {}, getStatus: () => STATUS } as unknown as SyncClient;
  return render(
    <MemoryRouter>
      <ClientProvider client={client}>
        <SessionControlContext.Provider value={{ logout: async () => {} }}>
          <SessionContext.Provider value={{ client, auth: AUTH, store }}>
            <AppShell />
          </SessionContext.Provider>
        </SessionControlContext.Provider>
      </ClientProvider>
    </MemoryRouter>,
  );
}

describe("brand mark in the shell", () => {
  let store: TtStore;
  beforeEach(async () => {
    store = await memoryStore();
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("desktop top bar shows the mark and no tt text", () => {
    stubPhone(false);
    renderShell(store);
    const bar = screen.getByRole("banner");
    expect(within(bar).getByRole("img", { name: "tt" })).toBeInTheDocument();
    expect(screen.queryByText("tt", { exact: true })).toBeNull();
  });

  it("phone header shows the mark and no tt text", () => {
    stubPhone(true);
    renderShell(store);
    expect(screen.getByRole("img", { name: "tt" })).toHaveAttribute("width", "18");
    expect(screen.queryByText("tt", { exact: true })).toBeNull();
  });
});
