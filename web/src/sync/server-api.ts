// Read-only server status: `/api/health` (no session) and `/api/peers`
// (bearer token). Both return null when unreachable or rejected, so callers
// keep their last result instead of showing an error page.

import type { Auth } from "./protocol.ts";

export interface Health {
  version: string;
  /** Client protocol version; 1 for servers from before the field. */
  protocol: number;
  /** The server's `--public-url`, if set. */
  public_url: string | null;
  server: { id: string; name: string };
  /** `needs-decision` until `tt-server init` or `tt-server peer join` has run. */
  setup: "ready" | "needs-decision";
}

export type PeerState = "online" | "offline" | "syncing" | "error";

export interface Peer {
  id: string;
  name: string;
  state: PeerState;
  /** Documents not yet converged with this peer. */
  pending: number;
  /** UTC seconds. */
  last_seen: number | null;
  address: string | null;
  public_url: string | null;
  error: string | null;
}

export interface Peers {
  server: { id: string; name: string | null; public_url?: string | null };
  peers: Peer[];
}

/** The server's health; its host stands in for a missing name. */
export async function fetchHealth(server: string, signal?: AbortSignal): Promise<Health | null> {
  try {
    const response = await fetch(`${server}/api/health`, { signal, cache: "no-store" });
    if (!response.ok) return null;
    const body = (await response.json()) as {
      version?: string;
      protocol?: number;
      public_url?: string | null;
      server?: { id?: string; name?: string | null } | null;
      setup?: string;
    };
    return {
      version: body.version ?? "",
      protocol: typeof body.protocol === "number" ? body.protocol : 1,
      public_url: body.public_url ?? null,
      server: { id: body.server?.id ?? "", name: body.server?.name || new URL(server).host },
      // Servers from before the setup field are set up by definition.
      setup: body.setup === "needs-decision" ? "needs-decision" : "ready",
    };
  } catch {
    return null;
  }
}

export async function fetchPeers(auth: Pick<Auth, "server" | "token">, signal?: AbortSignal): Promise<Peers | null> {
  try {
    const response = await fetch(`${auth.server}/api/peers`, { signal, headers: { authorization: `Bearer ${auth.token}` }, cache: "no-store" });
    if (!response.ok) return null;
    const body = (await response.json()) as Partial<Peers>;
    return Array.isArray(body.peers) && body.server ? { server: body.server, peers: body.peers } : null;
  } catch {
    return null;
  }
}
