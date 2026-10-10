import { describe, expect, it } from "vitest";

import type { Endpoint } from "@/sync/protocol";

import { memberState, memberStateLabel, memberSummary } from "./member-state.ts";

const member = (name: string, over: Partial<Endpoint> = {}): Endpoint => ({
  server_id: `${name}-id`,
  name,
  public_url: `https://${name}.example`,
  last_ok: null,
  ...over,
});

describe("member state", () => {
  const current = member("laptop-b");

  it("derives each state", () => {
    expect(memberState(current, current)).toBe("connected");
    expect(memberState(member("a", { reachable: true }), current)).toBe("reachable");
    expect(memberState(member("a", { reachable: false }), current)).toBe("unreachable");
    expect(memberState(member("a"), current)).toBe("unreachable");
    expect(memberState(member("a", { incompatible: true, reachable: true }), current)).toBe("skipped");
    // Not connected: no member is connected.
    expect(memberState(current, null)).toBe("unreachable");
  });

  it("labels the skipped state with the version", () => {
    expect(memberStateLabel(member("old", { version: "1.2.0" }), "skipped")).toBe("Skipped: incompatible version (1.2.0)");
    expect(memberStateLabel(member("old"), "skipped")).toBe("Skipped: incompatible version (unknown)");
    expect(memberStateLabel(current, "connected")).toBe("Connected");
  });

  it("summarizes mixed members, leaving out the one in use and skipped ones", () => {
    const list = [current, member("laptop-a", { reachable: true }), member("desk", { reachable: true }), member("pi", { reachable: false }), member("nas", { incompatible: true })];
    expect(memberSummary(list, current)).toEqual({ reachable: ["laptop-a", "desk"], unreachable: ["pi"] });
    expect(memberSummary([current], current)).toEqual({ reachable: [], unreachable: [] });
  });
});
