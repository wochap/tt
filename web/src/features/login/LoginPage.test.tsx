import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { applyFlavor, type Flavor } from "@/lib/theme";

import { LoginPage } from "./LoginPage.tsx";

const SERVER = { id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" };

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

/** `/api/health` names the server; `/api/login` answers with `login`. */
function mockServer(login: () => Response, health: () => Response = () => json(200, { ok: true, state: "Ready", server: SERVER })) {
  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const url = String(input);
    if (url.endsWith("/api/health")) return health();
    if (url.endsWith("/api/login")) return login();
    throw new Error(`unexpected fetch ${url}`);
  });
}

async function signIn(username: string, password: string) {
  await userEvent.type(screen.getByRole("textbox", { name: "Username" }), username);
  await userEvent.type(screen.getByLabelText("Password"), password);
  await userEvent.click(screen.getByRole("button", { name: /Sign in/ }));
}

describe.each<Flavor>(["mocha", "latte"])("LoginPage (%s)", (flavor) => {
  beforeEach(() => {
    applyFlavor(flavor);
    localStorage.clear();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("titles the form with the server name from /api/health and shows the URL", async () => {
    mockServer(() => json(500, {}));
    render(<LoginPage onLogin={() => {}} />);
    expect(await screen.findByRole("heading", { name: "Sign in to laptop-a" })).toBeInTheDocument();
    expect(screen.getByTestId("login-server")).toHaveTextContent(location.origin);
  });

  it("says one sign-in works on every paired server, under Sign in", async () => {
    mockServer(() => json(500, {}));
    render(<LoginPage onLogin={() => {}} />);
    const line = await screen.findByText("Works on all paired servers");
    const button = screen.getByRole("button", { name: /^Sign in/ });
    expect(button.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("falls back to the host when the server is unreachable", async () => {
    mockServer(
      () => json(500, {}),
      () => {
        throw new TypeError("offline");
      },
    );
    render(<LoginPage onLogin={() => {}} />);
    expect(screen.getByRole("heading", { name: `Sign in to ${location.host}` })).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(screen.getByRole("heading", { name: `Sign in to ${location.host}` })).toBeInTheDocument();
  });

  it("stores the server identity from a successful login", async () => {
    mockServer(() => json(200, { token: "tok", index_doc: "idx", user: { id: "u", name: "wochap" }, server: SERVER }));
    const onLogin = vi.fn();
    render(<LoginPage onLogin={onLogin} />);
    await signIn("wochap", "secret");
    await waitFor(() => expect(onLogin).toHaveBeenCalled());
    expect(onLogin.mock.calls[0]![0]).toMatchObject({ token: "tok", indexDoc: "idx", identity: SERVER });
  });

  it("wrong password: the red inline error, no conflict banner", async () => {
    mockServer(() => json(401, { error: "invalid username or password" }));
    const onLogin = vi.fn();
    render(<LoginPage onLogin={onLogin} />);
    await signIn("wochap", "wrong");
    expect(await screen.findByRole("alert")).toHaveTextContent("Invalid username or password");
    expect(screen.queryByTestId("login-conflict")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Sign in/ })).toBeEnabled();
    expect(onLogin).not.toHaveBeenCalled();
  });

  it("409 account_conflict: the conflict banner, not the wrong-password error; stays on login", async () => {
    mockServer(() => json(409, { error: "account_conflict" }));
    const onLogin = vi.fn();
    render(<LoginPage onLogin={onLogin} />);
    await signIn("wochap", "secret");
    const banner = await screen.findByTestId("login-conflict");
    expect(banner).toHaveTextContent("This account name is also used on another server. Ask the admin to rename it.");
    expect(banner).toHaveTextContent("tt-server user rename wochap <new-name>");
    expect(screen.getAllByRole("alert")).toEqual([banner]);
    expect(screen.queryByText("Invalid username or password")).not.toBeInTheDocument();
    expect(onLogin).not.toHaveBeenCalled();
    // Retrying cannot fix it: Sign in stays disabled until the username changes.
    expect(screen.getByRole("button", { name: /Sign in/ })).toBeDisabled();
    expect(screen.getByText("blocked")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Username" }), "2");
    expect(screen.getByRole("button", { name: /Sign in/ })).toBeEnabled();
  });
});
