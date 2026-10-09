import { afterEach, describe, expect, it, vi } from "vitest";

import { loadAuth, refreshMe, saveAuth, updateAuth } from "./auth.ts";
import type { Auth } from "./protocol.ts";

const AUTH: Auth = { server: "https://laptop-a.example", token: "t", indexDoc: "i", user: { id: "u", name: "wochap" } };

describe("session identity", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
    sessionStorage.clear();
  });

  it("refreshMe adds the server identity and updateAuth keeps the storage choice", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ user: { id: "u", name: "wochap" }, index_doc: "i", server: { id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" } })),
    );
    saveAuth(AUTH, false);
    const next = await refreshMe(AUTH);
    expect(next?.identity).toEqual({ id: "7f3a9c21qrstuvwxyz234567ab", name: "laptop-a" });
    updateAuth(next!);
    expect(localStorage.getItem("tt.auth")).toBeNull();
    expect(loadAuth()?.identity?.name).toBe("laptop-a");
  });

  it("refreshMe offline returns null so the stored identity stays", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new TypeError("offline"));
    expect(await refreshMe(AUTH)).toBeNull();
  });
});
