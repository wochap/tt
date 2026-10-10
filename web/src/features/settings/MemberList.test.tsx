import { act, cleanup, render, screen, within } from "@testing-library/react";
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
const D = "https://travel-pi.tail3e1.ts.net";
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
  { server_id: "e0f4192dabcdefghijklmnopqr", name: "old-nas", public_url: C, last_ok: null, incompatible: true, version: "1.2.0" },
];

const REACHABLE: Endpoint[] = [{ ...MEMBERS[0]!, reachable: true }, MEMBERS[1]!];
const UNREACHABLE: Endpoint[] = [{ ...MEMBERS[0]!, reachable: false }, MEMBERS[1]!];

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

  it("Settings lists each member's state, short id and URL, the one in use marked", () => {
    const members = [{ ...MEMBERS[0]!, reachable: true }, MEMBERS[1]!, { server_id: "5be7a3c2abcdefghijklmnopqr", name: "travel-pi", public_url: D, last_ok: null, reachable: false }, MEMBERS[2]!];
    renderWithSession(<MemberList status={{ ...status("synced", MEMBERS[1]!), members }} />);
    const rows = screen.getAllByTestId("member-row");
    expect(rows.map((row) => within(row).getByTestId("member-state").textContent)).toEqual([
      "Reachable",
      "Connected",
      "Unreachable",
      "Skipped: incompatible version (1.2.0)",
    ]);
    expect(rows[0]).toHaveTextContent("7f3a9c21");
    expect(rows[0]).toHaveTextContent(A);
    expect(within(rows[1]!).getByTestId("member-current")).toHaveTextContent("in use");
    expect(rows[1]).toHaveAttribute("data-current", "true");
    expect(screen.getAllByTestId("member-current")).toHaveLength(1);
    expect(screen.getByText(/^Read-only\. The app picks a member by itself/)).toBeInTheDocument();
  });

  it("the tooltip lists reachable members", () => {
    renderWithSession(<SyncDetails status={{ ...status("synced", MEMBERS[1]!), members: REACHABLE }} now={NOW} />);
    expect(screen.getByTestId("sync-details")).toHaveTextContent("Also reachable: laptop-a");
    expect(screen.queryByTestId("sync-other-members")).toBeNull();
  });

  it("the tooltip lists unreachable members", () => {
    renderWithSession(<SyncDetails status={{ ...status("synced", MEMBERS[1]!), members: UNREACHABLE }} now={NOW} />);
    expect(screen.getByTestId("sync-details")).toHaveTextContent("Other members: laptop-a (unreachable)");
    expect(screen.queryByTestId("sync-also-reachable")).toBeNull();
  });

  it("Settings before the first connection", () => {
    renderWithSession(<MemberList status={{ state: "connecting", pending: 0, lastSync: null, mode: "shared" }} />);
    expect(screen.queryByTestId("members-list")).toBeNull();
  });
});

describe("failover notice", () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW }));
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("reads Switched to <member> with an accent tint for 4 s, then the synced line", () => {
    renderWithSession(<SyncStatus status={{ ...status("synced", MEMBERS[1]!), switchedAt: NOW }} />);
    const line = screen.getByTestId("sync-status");
    expect(line).toHaveTextContent(/^Switched to laptop-b$/);
    expect(line).toHaveAttribute("data-switched", "true");
    expect(screen.queryByRole("status")).toBeNull();
    act(() => vi.advanceTimersByTime(3_900));
    expect(line).toHaveTextContent("Switched to laptop-b");
    act(() => vi.advanceTimersByTime(200));
    expect(line).toHaveTextContent(/^Synced · laptop-b/);
    expect(line).not.toHaveAttribute("data-switched");
  });

  it("the compact chip is tinted and keeps the member name", () => {
    renderWithSession(<SyncStatus compact status={{ ...status("synced", MEMBERS[1]!), switchedAt: NOW }} />);
    const chip = screen.getByTestId("sync-status");
    expect(chip).toHaveTextContent("laptop-b");
    expect(chip).toHaveAttribute("data-switched", "true");
    expect(chip).toHaveAttribute("aria-label", "Switched to laptop-b");
  });

  it("an old switch shows nothing", () => {
    renderWithSession(<SyncStatus status={{ ...status("synced", MEMBERS[1]!), switchedAt: NOW - 10_000 }} />);
    expect(screen.getByTestId("sync-status")).not.toHaveAttribute("data-switched");
  });
});
