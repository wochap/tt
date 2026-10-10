import type { Endpoint } from "@/sync/protocol";

export type MemberState = "connected" | "reachable" | "unreachable" | "skipped";

type Current = Pick<Endpoint, "public_url"> | null | undefined;

/** One member's state for Settings and the sync tooltip; never checked counts as unreachable. */
export function memberState(endpoint: Endpoint, current: Current): MemberState {
  if (current && endpoint.public_url === current.public_url) return "connected";
  if (endpoint.incompatible) return "skipped";
  return endpoint.reachable === true ? "reachable" : "unreachable";
}

export function memberStateLabel(endpoint: Endpoint, state: MemberState): string {
  switch (state) {
    case "connected":
      return "Connected";
    case "reachable":
      return "Reachable";
    case "unreachable":
      return "Unreachable";
    case "skipped":
      return `Skipped: incompatible version (${endpoint.version || "unknown"})`;
  }
}

/** The tooltip's lines about the members not in use: those answering, and those that did not. */
export function memberSummary(members: Endpoint[], current: Current): { reachable: string[]; unreachable: string[] } {
  const reachable: string[] = [];
  const unreachable: string[] = [];
  for (const member of members) {
    const state = memberState(member, current);
    if (state === "reachable") reachable.push(member.name);
    else if (state === "unreachable") unreachable.push(member.name);
  }
  return { reachable, unreachable };
}
