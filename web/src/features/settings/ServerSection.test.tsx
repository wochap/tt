import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SessionContext } from "@/data/react";
import type { TtStore } from "@/data/store";
import { applyFlavor, type Flavor } from "@/lib/theme";
import type { SyncClient } from "@/sync/client";
import type { Auth } from "@/sync/protocol";
import type { Peer } from "@/sync/server-api";

import { forgetServerStatus, ServerSection } from "./ServerSection.tsx";

const AUTH: Auth = {
  server: "https://laptop-a.tail3e1.ts.net",
  token: "tok",
  indexDoc: "i",
  user: { id: "u", name: "wochap" },
  identity: { id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" },
};
const SERVER = { id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" };
const nowSeconds = () => Math.floor(Date.now() / 1000);

function peer(over: Partial<Peer>): Peer {
  return { id: "c41e08b7abcdefghijklmnopqr", name: "laptop-b", state: "online", pending: 0, last_seen: nowSeconds() - 120, address: "100.64.0.2:7443", public_url: null, error: null, ...over };
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

/** Answers `/api/health` and `/api/peers`; `peers` may throw to play an unreachable server. */
function mockServer(peers: () => Response) {
  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = String(input);
    if (url.endsWith("/api/health")) return json(200, { ok: true, version: "0.1.0", sessions: 1, state: "Ready", server: SERVER, setup: "ready" });
    if (url.endsWith("/api/peers")) {
      if (new Headers(init?.headers).get("authorization") !== `Bearer ${AUTH.token}`) return json(401, {});
      return peers();
    }
    throw new Error(`unexpected fetch ${url}`);
  });
}

function renderSection(refreshMs?: number) {
  const session = { client: {} as SyncClient, auth: AUTH, store: {} as TtStore };
  return render(
    <SessionContext.Provider value={session}>
      <ServerSection refreshMs={refreshMs} />
    </SessionContext.Provider>,
  );
}

function phoneMedia(matches: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: matches && query === "(max-width: 640px)",
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe.each<Flavor>(["mocha", "latte"])("Settings server section (%s)", (flavor) => {
  beforeEach(() => {
    applyFlavor(flavor);
    forgetServerStatus();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("shows this server's identity and lists peers in a table on desktop", async () => {
    mockServer(() =>
      json(200, {
        server: SERVER,
        peers: [
          peer({}),
          peer({ id: "9a2d51f0abcdefghijklmnopqr", name: "desk-mini", state: "syncing", pending: 3, last_seen: nowSeconds() }),
          peer({ id: "e0f4192dabcdefghijklmnopqr", name: "old-nas", state: "error", error: "version 1.2.0 too old", address: null }),
        ],
      }),
    );
    renderSection();
    const table = await screen.findByTestId("peers-table");
    expect(screen.getByTestId("server-name")).toHaveTextContent("laptop-a");
    expect(screen.getByTestId("server-id")).toHaveTextContent(/^7f3a9c21$/);
    expect(screen.getByRole("button", { name: "Copy 7f3a9c21" })).toBeInTheDocument();
    expect(await screen.findByTestId("server-version")).toHaveTextContent("tt-server 0.1.0");
    const rows = within(table).getAllByTestId("peer-row");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("laptop-b");
    expect(rows[0]).toHaveTextContent("c41e08b7");
    expect(rows[0]).toHaveTextContent("Online · in sync");
    expect(rows[0]).toHaveTextContent("100.64.0.2:7443");
    expect(rows[0]).toHaveTextContent("2m ago");
    expect(rows[1]).toHaveTextContent("Syncing · 3 docs pending");
    expect(rows[1]).toHaveTextContent("now");
    expect(rows[2]).toHaveTextContent("Error · version 1.2.0 too old");
    expect(screen.getByText("3 peers · 2 reachable")).toBeInTheDocument();
    expect(screen.queryByTestId("peers-stale")).not.toBeInTheDocument();
    expect(screen.queryByTestId("peers-list")).not.toBeInTheDocument();
  });

  it("phone: the same peers as a stacked list", async () => {
    phoneMedia(true);
    mockServer(() => json(200, { server: SERVER, peers: [peer({ state: "offline" })] }));
    renderSection();
    const list = await screen.findByTestId("peers-list");
    expect(within(list).getByTestId("peer-row")).toHaveTextContent(/laptop-b.*Offline.*100\.64\.0\.2:7443.*2m ago/);
    expect(screen.queryByTestId("peers-table")).not.toBeInTheDocument();
  });

  it("unpaired: the empty state with both CLI commands", async () => {
    mockServer(() => json(200, { server: SERVER, peers: [] }));
    renderSection();
    const empty = await screen.findByTestId("peers-empty");
    expect(empty).toHaveTextContent("Not paired with other servers");
    expect(empty).toHaveTextContent("tt-server peer invite");
    expect(empty).toHaveTextContent("tt-server peer join <code>");
    expect(screen.queryByTestId("peer-row")).not.toBeInTheDocument();
  });

  it("server unreachable on refresh: keeps the last list, marked stale, no error page", async () => {
    let up = true;
    mockServer(() => {
      if (!up) throw new TypeError("offline");
      return json(200, { server: SERVER, peers: [peer({})] });
    });
    renderSection(30);
    expect(await screen.findByTestId("peer-row")).toHaveTextContent("laptop-b");
    up = false;
    expect(await screen.findByTestId("peers-stale")).toHaveTextContent("Stale · laptop-a unreachable");
    expect(screen.getByTestId("peer-row")).toHaveTextContent("Online · in sync");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // Reopening Settings while still offline shows the cached list, stale.
    cleanup();
    renderSection(30);
    expect(screen.getByTestId("peer-row")).toHaveTextContent("laptop-b");
    expect(screen.getByTestId("peers-stale")).toBeInTheDocument();
  });

  it("does not poll while the section is off screen", async () => {
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(private callback: (entries: { isIntersecting: boolean }[]) => void) {}
        observe() {
          this.callback([{ isIntersecting: false }]);
        }
        disconnect() {}
      },
    );
    const fetch = mockServer(() => json(200, { server: SERVER, peers: [] }));
    renderSection(10);
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(fetch).not.toHaveBeenCalled();
    expect(screen.getByText("Loading peers…")).toBeInTheDocument();
  });
});
