// The member endpoint list and the rotation over it. Every member of a root
// accepts the same token, so the worker connects to whichever member is
// reachable: the last one that synced first, then the others by most recent
// success, then members never reached. Each attempt checks the member's
// health and protocol, takes a ticket from that member, and opens its `/sync`.

import { type Auth, type Endpoint, SUPPORTED_PROTOCOLS, serverName } from "./protocol.ts";
import type { Peers } from "./server-api.ts";
import { Unauthorized } from "./ticket-socket.ts";

/** Per-attempt timeout of the health check and of the ticket request. */
export const ATTEMPT_TIMEOUT_MS = 5_000;

const sameMember = (a: Pick<Endpoint, "server_id" | "public_url">, b: Pick<Endpoint, "server_id" | "public_url">) =>
  (a.server_id !== "" && a.server_id === b.server_id) || a.public_url === b.public_url;

/** The origin of a client URL, or null when it is not an http(s) URL. */
export function urlOrigin(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.origin : null;
  } catch {
    return null;
  }
}

/** The stored list, or just the sign-in server; the sign-in server is always in it. */
export function seedEndpoints(auth: Auth): Endpoint[] {
  const list = auth.endpoints ?? [];
  if (list.some((endpoint) => endpoint.public_url === auth.server)) return list;
  return [...list, { server_id: auth.identity?.id ?? "", name: serverName(auth), public_url: auth.server, last_ok: null }];
}

/**
 * The list after a successful connection to `connected`: that member at the
 * URL that worked, every other member `/api/peers` lists with a client URL
 * (revoked members are not listed, so they drop out), and the sign-in server.
 * Success times and incompatible markers carry over.
 */
export function refreshEndpoints(list: Endpoint[], peers: Peers, connected: Endpoint, signIn: string): Endpoint[] {
  const next: Endpoint[] = [];
  const add = (entry: Pick<Endpoint, "server_id" | "name" | "public_url">) => {
    if (next.some((known) => sameMember(known, entry))) return;
    const old = list.find((known) => sameMember(known, entry));
    next.push({ ...entry, last_ok: old?.last_ok ?? null, ...(old?.incompatible ? { incompatible: true } : {}) });
  };
  add({ server_id: peers.server.id, name: peers.server.name ?? connected.name, public_url: connected.public_url });
  for (const peer of peers.peers) {
    const url = peer.public_url ? urlOrigin(peer.public_url) : null;
    if (url) add({ server_id: peer.id, name: peer.name, public_url: url });
  }
  const own = list.find((known) => known.public_url === signIn);
  if (!next.some((known) => known.public_url === signIn || (own?.server_id && known.server_id === own.server_id))) {
    next.push(own ?? { server_id: "", name: new URL(signIn).host, public_url: signIn, last_ok: null });
  }
  return next;
}

/** Most recent success first, never-reached members last (in list order). */
export function orderEndpoints(list: Endpoint[]): Endpoint[] {
  return list
    .map((endpoint, index) => ({ endpoint, index }))
    .sort((a, b) => (b.endpoint.last_ok ?? -1) - (a.endpoint.last_ok ?? -1) || a.index - b.index)
    .map(({ endpoint }) => endpoint);
}

/** Records a successful connection to the member at `url`. */
export function markOk(list: Endpoint[], url: string, at: number): Endpoint[] {
  return list.map((endpoint) => (endpoint.public_url === url ? { ...endpoint, last_ok: at, incompatible: undefined } : endpoint));
}

export function markIncompatible(list: Endpoint[], url: string, incompatible: boolean): Endpoint[] {
  return list.map((endpoint) => (endpoint.public_url === url ? { ...endpoint, incompatible: incompatible || undefined } : endpoint));
}

/** The `/sync` URL of a member with a ticket. */
export function syncUrl(server: string, ticket: string): string {
  const url = new URL("/sync", server);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.searchParams.set("ticket", ticket);
  return url.toString();
}

/** A member answered with a protocol version this bundle does not speak. */
export class Incompatible extends Error {
  constructor(readonly protocol: number) {
    super(`protocol ${protocol} not supported`);
    this.name = "Incompatible";
  }
}

/** No member could be reached in a full pass. */
export class NoMemberReachable extends Error {
  constructor() {
    super("no member reachable");
    this.name = "NoMemberReachable";
  }
}

/** A compatible member answered its health check but gave no ticket for another reason than 401. */
class TicketError extends Error {}

export interface RotationOptions {
  endpoints: () => Endpoint[];
  token: () => string;
  timeoutMs?: number;
  supported?: { min: number; max: number };
  fetch?: typeof fetch;
  /** A member's health check found it (in)compatible. */
  onCompatibility?: (endpoint: Endpoint, incompatible: boolean) => void;
}

/**
 * Picks the member for the next connection. `url()` tries the members not yet
 * tried in this pass, in order, and resolves with the first `/sync` URL it
 * gets; when every member failed it throws (the socket then backs off) and
 * the next call starts a new pass. `closed()` tells it the socket closed:
 * after a drop the pass restarts from the other members, so the next member
 * is tried at once instead of after a backoff.
 */
export class MemberRotation {
  readonly #options: RotationOptions;
  #tried = new Set<string>();
  #current: Endpoint | null = null;

  constructor(options: RotationOptions) {
    this.#options = options;
  }

  /** The member of the last URL handed out (the one connected to while the socket is open). */
  get current(): Endpoint | null {
    return this.#current;
  }

  async url(): Promise<string> {
    let rejected = 0;
    let failed = 0;
    for (const endpoint of orderEndpoints(this.#options.endpoints())) {
      if (this.#tried.has(endpoint.public_url)) continue;
      this.#tried.add(endpoint.public_url);
      try {
        const url = await this.#attempt(endpoint);
        this.#current = endpoint;
        return url;
      } catch (error) {
        if (error instanceof Unauthorized) rejected++;
        else if (error instanceof TicketError) failed++;
      }
    }
    this.#tried.clear();
    this.#current = null;
    // Every member that answered refused the token: it was revoked.
    if (rejected > 0 && failed === 0) throw new Unauthorized();
    throw new NoMemberReachable();
  }

  /** The socket closed; `opened` when it had connected. True when a member is left to try at once. */
  closed(opened: boolean): boolean {
    if (opened) this.#tried = new Set(this.#current ? [this.#current.public_url] : []);
    this.#current = null;
    const left = this.#options.endpoints().some((endpoint) => !this.#tried.has(endpoint.public_url));
    if (!left) this.#tried.clear();
    return left;
  }

  /** The socket connected: the pass is over. */
  connected(): void {
    this.#tried.clear();
  }

  async #attempt(endpoint: Endpoint): Promise<string> {
    const fetcher = this.#options.fetch ?? fetch;
    const timeout = this.#options.timeoutMs ?? ATTEMPT_TIMEOUT_MS;
    const supported = this.#options.supported ?? SUPPORTED_PROTOCOLS;
    const health = await fetcher(`${endpoint.public_url}/api/health`, { cache: "no-store", signal: AbortSignal.timeout(timeout) });
    if (!health.ok) throw new Error(`health: HTTP ${health.status}`);
    const body = (await health.json()) as { protocol?: unknown };
    // Servers from before the protocol field speak version 1.
    const protocol = typeof body.protocol === "number" ? body.protocol : 1;
    const incompatible = protocol < supported.min || protocol > supported.max;
    if (incompatible || endpoint.incompatible) this.#options.onCompatibility?.(endpoint, incompatible);
    if (incompatible) throw new Incompatible(protocol);
    let response: Response;
    try {
      response = await fetcher(`${endpoint.public_url}/api/ws-ticket`, {
        method: "POST",
        headers: { authorization: `Bearer ${this.#options.token()}` },
        signal: AbortSignal.timeout(timeout),
      });
    } catch {
      throw new TicketError("ws-ticket: unreachable");
    }
    if (response.status === 401) throw new Unauthorized();
    if (!response.ok) throw new TicketError(`ws-ticket: HTTP ${response.status}`);
    const { ticket } = (await response.json()) as { ticket: string };
    return syncUrl(endpoint.public_url, ticket);
  }
}
