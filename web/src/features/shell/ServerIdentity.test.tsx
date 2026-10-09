import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { ClientProvider, SessionContext } from "@/data/react";
import { SessionControlContext } from "@/data/session-control";
import type { TtStore } from "@/data/store";
import { applyFlavor, type Flavor } from "@/lib/theme";
import type { SyncClient } from "@/sync/client";
import type { Auth, SyncStatus as Status } from "@/sync/protocol";

import { SyncDetails, SyncStatus } from "./SyncStatus.tsx";
import { AccountMenu } from "./TopBar.tsx";
import { UiProvider } from "./ui-state.tsx";

const AUTH: Auth = {
  server: "https://laptop-a.tail3e1.ts.net",
  token: "t",
  indexDoc: "i",
  user: { id: "u", name: "wochap" },
  identity: { id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" },
};

const NOW = Date.parse("2026-10-09T15:41:42Z");

function status(state: Status["state"], pending = 0): Status {
  return { state, pending, lastSync: NOW - 14_000, mode: "shared" };
}

function renderShell(ui: ReactNode, current: Status, auth: Auth = AUTH) {
  const client = { subscribe: () => () => {}, getStatus: () => current } as unknown as SyncClient;
  return render(
    <MemoryRouter>
      <ClientProvider client={client}>
        <SessionControlContext.Provider value={{ logout: async () => {} }}>
          <SessionContext.Provider value={{ client, auth, store: {} as TtStore }}>
            <UiProvider>
              <TooltipProvider>{ui}</TooltipProvider>
            </UiProvider>
          </SessionContext.Provider>
        </SessionControlContext.Provider>
      </ClientProvider>
    </MemoryRouter>,
  );
}

describe.each<Flavor>(["mocha", "latte"])("server identity (%s)", (flavor) => {
  beforeEach(() => {
    applyFlavor(flavor);
    vi.useFakeTimers({ now: NOW, toFake: ["Date"] });
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("names the server in the status bar label", () => {
    expect(document.documentElement.dataset.theme).toBe(flavor);
    renderShell(<SyncStatus status={status("synced")} />, status("synced"));
    expect(screen.getByTestId("sync-status")).toHaveTextContent("Synced · laptop-a · 14s ago");
    cleanup();
    renderShell(<SyncStatus status={status("syncing", 3)} />, status("syncing", 3));
    expect(screen.getByTestId("sync-status")).toHaveTextContent("Syncing 3 changes · laptop-a");
  });

  it("offline: label and tooltip use the identity stored at login", () => {
    const fetch = vi.spyOn(globalThis, "fetch");
    renderShell(
      <>
        <SyncStatus status={status("offline", 5)} />
        <SyncDetails status={status("offline", 5)} now={NOW} />
      </>,
      status("offline", 5),
    );
    expect(screen.getByTestId("sync-status")).toHaveTextContent("laptop-a unreachable · 5 changes saved here");
    const details = screen.getByTestId("sync-details");
    expect(details).toHaveTextContent("Changes are saved on this device and sync when laptop-a is reachable.");
    expect(details).toHaveTextContent("7f3a9c21");
    expect(details).not.toHaveTextContent("7f3a9c21q");
    expect(details).toHaveTextContent("5 changes");
    expect(fetch).not.toHaveBeenCalled();
    fetch.mockRestore();
  });

  it("phone header shows the server name", () => {
    renderShell(<SyncStatus status={status("synced")} compact />, status("synced"));
    expect(screen.getByTestId("sync-status")).toHaveTextContent(/^laptop-a$/);
    expect(screen.getByTestId("sync-status")).toHaveAccessibleName("Synced · laptop-a · 14s ago");
  });

  it("falls back to the host for sessions stored before the identity existed", () => {
    renderShell(<SyncStatus status={status("offline")} />, status("offline"), { ...AUTH, identity: undefined });
    expect(screen.getByTestId("sync-status")).toHaveTextContent("laptop-a.tail3e1.ts.net unreachable");
  });

  it("account menu reads '<user> on <server>'", async () => {
    vi.useRealTimers();
    renderShell(<AccountMenu />, status("synced"));
    await userEvent.click(screen.getByRole("button", { name: "wochap on laptop-a" }));
    const identity = await screen.findByTestId("account-identity");
    expect(identity).toHaveTextContent("wochap on laptop-a");
    expect(identity).toHaveTextContent(/Synced · \d+[smh] ago/);
    expect(screen.getByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
  });
});
