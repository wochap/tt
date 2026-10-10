import { cleanup, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import type { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";
import type { Auth, Endpoint, SyncStatus as Status } from "@/sync/protocol";

import { SyncDetails, SyncStatus } from "../shell/SyncStatus.tsx";
import { MemberList } from "./MemberList.tsx";

const A = "https://laptop-a.tail3e1.ts.net";
const B = "https://laptop-b.tail3e1.ts.net";
const C = "https://old-nas.tail3e1.ts.net";
const NOW = Date.parse("2026-10-09T15:41:42Z");

const AUTH: Auth = {
  server: A,
  token: "t",
  indexDoc: "i",
  user: { id: "u", name: "wochap" },
  identity: { id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" },
};

const MEMBERS: Endpoint[] = [
  { server_id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a", public_url: A, last_ok: NOW - 120_000 },
  { server_id: "c41e08b7abcdefghijklmnopqr", name: "laptop-b", public_url: B, last_ok: NOW - 1_000 },
  { server_id: "e0f4192dabcdefghijklmnopqr", name: "old-nas", public_url: C, last_ok: null, incompatible: true },
];

function status(state: Status["state"], member: Endpoint | null, pending = 0): Status {
  return { state, pending, lastSync: NOW - 14_000, mode: "shared", member, members: MEMBERS };
}

function renderWithSession(ui: ReactNode) {
  const session = { client: {} as SyncClient, auth: AUTH, store: {} as TtStore };
  return render(
    <SessionContext.Provider value={session}>
      <TooltipProvider>{ui}</TooltipProvider>
    </SessionContext.Provider>,
  );
}

describe("current member", () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ["Date"] }));
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("the status bar names the member synced with and follows a failover without a reload", () => {
    const { rerender } = renderWithSession(<SyncStatus status={status("synced", MEMBERS[0]!)} />);
    expect(screen.getByTestId("sync-status")).toHaveTextContent("Synced · laptop-a · 14s ago");
    rerender(
      <SessionContext.Provider value={{ client: {} as SyncClient, auth: AUTH, store: {} as TtStore }}>
        <TooltipProvider>
          <SyncStatus status={status("synced", MEMBERS[1]!)} />
        </TooltipProvider>
      </SessionContext.Provider>,
    );
    expect(screen.getByTestId("sync-status")).toHaveTextContent("Synced · laptop-b · 14s ago");
  });

  it("the tooltip shows the current member's id", () => {
    renderWithSession(<SyncDetails status={status("synced", MEMBERS[1]!)} now={NOW} />);
    const details = screen.getByTestId("sync-details");
    expect(details).toHaveTextContent("laptop-b");
    expect(details).toHaveTextContent("c41e08b7");
    expect(details).toHaveTextContent("sync when one of your servers is reachable");
  });

  it("offline with several members: Offline with the queued count", () => {
    renderWithSession(<SyncStatus status={status("offline", null, 5)} />);
    expect(screen.getByTestId("sync-status")).toHaveTextContent(/^Offline · 5 changes saved here$/);
  });

  it("Settings lists the members with the current and incompatible ones marked", () => {
    renderWithSession(<MemberList status={status("synced", MEMBERS[1]!)} />);
    const rows = screen.getAllByTestId("member-row");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("laptop-a");
    expect(rows[0]).toHaveTextContent("last synced 2m ago");
    expect(within(rows[1]!).getByTestId("member-current")).toHaveTextContent("Syncing now");
    expect(rows[1]).toHaveAttribute("data-current", "true");
    expect(within(rows[2]!).getByTestId("member-incompatible")).toHaveTextContent("Incompatible version");
    expect(screen.getAllByTestId("member-current")).toHaveLength(1);
  });

  it("Settings before the first connection", () => {
    renderWithSession(<MemberList status={{ state: "connecting", pending: 0, lastSync: null, mode: "shared" }} />);
    expect(screen.queryByTestId("members-list")).toBeNull();
  });
});
