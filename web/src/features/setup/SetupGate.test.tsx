import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { applyFlavor, type Flavor } from "@/lib/theme";

import { LoginPage } from "../login/LoginPage.tsx";
import { SetupGate } from "./SetupGate.tsx";

const SERVER = "https://laptop-b.tail3e1.ts.net";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

/** `/api/health` reports `setup()`; a not-set-up server names itself by host. */
function mockHealth(setup: () => "ready" | "needs-decision") {
  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const url = String(input);
    if (!url.endsWith("/api/health")) throw new Error(`unexpected fetch ${url}`);
    const state = setup();
    return json(200, {
      ok: true,
      version: "0.1.0",
      sessions: 0,
      state: state === "ready" ? "Ready" : "NeedsDecision",
      server: { id: "c41e08b7abcdefghijklmnopqr", name: state === "ready" ? "laptop-b" : "laptop-b.tail3e1.ts.net" },
      setup: state,
    });
  });
}

function renderGate(recheckMs?: number) {
  return render(
    <SetupGate server={SERVER} recheckMs={recheckMs}>
      <LoginPage onLogin={() => {}} />
    </SetupGate>,
  );
}

describe.each<Flavor>(["mocha", "latte"])("not set up page (%s)", (flavor) => {
  beforeEach(() => {
    applyFlavor(flavor);
    localStorage.clear();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("needs-decision: the not-set-up page with copyable commands, no login form", async () => {
    mockHealth(() => "needs-decision");
    renderGate();
    expect(await screen.findByRole("heading", { name: "laptop-b.tail3e1.ts.net is not set up" })).toBeInTheDocument();
    expect(screen.getAllByTestId("setup-command").map((c) => c.textContent)).toEqual(["tt-server init", "tt-server peer join <code>"]);
    expect(screen.queryByRole("form", { name: "Sign in" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Username" })).not.toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Copy tt-server init" }));
    expect(await navigator.clipboard.readText()).toBe("tt-server init");
    expect(screen.getByRole("button", { name: "Copy tt-server init" })).toHaveTextContent("Copied");
  });

  it("switches to the login form once health reports ready", async () => {
    let setup: "ready" | "needs-decision" = "needs-decision";
    const fetch = mockHealth(() => setup);
    renderGate(30);
    expect(await screen.findByTestId("not-set-up")).toBeInTheDocument();
    // Still not set up on the next checks: the page stays.
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(fetch.mock.calls.length).toBeGreaterThan(1);
    expect(screen.getByTestId("not-set-up")).toBeInTheDocument();
    setup = "ready";
    expect(await screen.findByRole("form", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByTestId("not-set-up")).not.toBeInTheDocument();
  });

  it("re-checks every 10 s by default", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    try {
      const fetch = mockHealth(() => "needs-decision");
      renderGate();
      await vi.waitFor(() => expect(screen.getByTestId("not-set-up")).toBeInTheDocument());
      const after = fetch.mock.calls.length;
      await vi.advanceTimersByTimeAsync(9_000);
      expect(fetch.mock.calls.length).toBe(after);
      await vi.advanceTimersByTimeAsync(1_000);
      expect(fetch.mock.calls.length).toBe(after + 1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("ready: the login form, as before", async () => {
    mockHealth(() => "ready");
    renderGate();
    expect(await screen.findByRole("form", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByTestId("not-set-up")).not.toBeInTheDocument();
  });
});
