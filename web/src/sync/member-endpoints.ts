// The worker's endpoint list for the signed-in session: seeded from the
// stored session (or the sign-in server), refreshed from `/api/peers` of the
// member just connected to, and handed to the tabs to persist with the
// session. Signing out drops it. While connected, the members not in use get
// a health check about once a minute, so Settings and the sync tooltip can
// tell reachable members from unreachable ones.

import { ATTEMPT_TIMEOUT_MS, markIncompatible, markOk, markProbed, MemberRotation, refreshEndpoints, seedEndpoints } from "./endpoints.ts";
import type { Auth, Endpoint } from "./protocol.ts";
import { fetchPeers } from "./server-api.ts";

/** Interval of the health checks of members not in use. */
export const PROBE_INTERVAL_MS = 60_000;

export interface ProbeOptions {
  fetch?: typeof fetch;
  intervalMs?: number;
  timeoutMs?: number;
  /** Jitter source in [0, 1). */
  random?: () => number;
  /** False while the device has no network. */
  online?: () => boolean;
}

export class MemberEndpoints {
  readonly #persist: (token: string, endpoints: Endpoint[]) => void;
  readonly #probe: ProbeOptions;
  #list: Endpoint[] = [];
  #auth: Auth | null = null;
  #rotation: MemberRotation | undefined;
  #onChange: () => void = () => {};
  #probeTimer: ReturnType<typeof setTimeout> | undefined;
  #lastConnected: string | null = null;
  #switchedAt: number | null = null;

  constructor(persist: (token: string, endpoints: Endpoint[]) => void, probe: ProbeOptions = {}) {
    this.#persist = persist;
    this.#probe = probe;
  }

  get list(): Endpoint[] {
    return this.#list;
  }

  /** The member of the current or last connection attempt, as the list knows it now. */
  get current(): Endpoint | null {
    const current = this.#rotation?.current;
    if (!current) return null;
    return this.#list.find((endpoint) => endpoint.public_url === current.public_url) ?? current;
  }

  /** When this session's connection moved to another member (ms since epoch); null until it did. */
  get switchedAt(): number | null {
    return this.#switchedAt;
  }

  /** Starts over for a new session (or none); `onChange` hears about compatibility and reachability changes. */
  reset(auth: Auth | null, onChange: () => void): MemberRotation | undefined {
    this.disconnected();
    this.#auth = auth;
    this.#onChange = onChange;
    this.#lastConnected = null;
    this.#switchedAt = null;
    this.#list = auth ? seedEndpoints(auth) : [];
    this.#rotation = auth
      ? new MemberRotation({
          endpoints: () => this.#list,
          token: () => auth.token,
          onCompatibility: (endpoint, incompatible, version) => {
            this.#list = markIncompatible(this.#list, endpoint.public_url, incompatible, version);
            onChange();
          },
        })
      : undefined;
    return this.#rotation;
  }

  /** The socket of `auth`'s session connected: records the success, refreshes the list from that member and starts the health checks. */
  async connected(auth: Auth): Promise<void> {
    const rotation = this.#rotation;
    const member = rotation?.current;
    if (!rotation || !member || this.#auth !== auth) return;
    rotation.connected();
    // The first connection of a session is no switch.
    if (this.#lastConnected !== null && this.#lastConnected !== member.public_url) this.#switchedAt = Date.now();
    this.#lastConnected = member.public_url;
    this.#list = markOk(this.#list, member.public_url, Date.now());
    const peers = await fetchPeers({ server: member.public_url, token: auth.token });
    if (this.#auth !== auth) return;
    if (peers) this.#list = refreshEndpoints(this.#list, peers, member, auth.server);
    this.#persist(auth.token, this.#list);
    this.#schedule(0);
  }

  /** The socket closed (or the session ended): the health checks stop until the next connection. */
  disconnected(): void {
    clearTimeout(this.#probeTimer);
    this.#probeTimer = undefined;
  }

  /** Checks the health of every member not in use; skipped while offline or signed out. */
  async probeOthers(): Promise<void> {
    const auth = this.#auth;
    const current = this.#rotation?.current;
    if (!auth || !current || !(this.#probe.online?.() ?? navigator.onLine)) return;
    const fetcher = this.#probe.fetch ?? fetch;
    const timeout = this.#probe.timeoutMs ?? ATTEMPT_TIMEOUT_MS;
    const others = this.#list.filter((endpoint) => endpoint.public_url !== current.public_url);
    const results = await Promise.all(
      others.map(async (endpoint) => {
        try {
          const response = await fetcher(`${endpoint.public_url}/api/health`, { cache: "no-store", signal: AbortSignal.timeout(timeout) });
          if (!response.ok) return { url: endpoint.public_url, health: null };
          const body = (await response.json()) as { version?: unknown; protocol?: unknown };
          return {
            url: endpoint.public_url,
            // Servers from before the protocol field speak version 1.
            health: { version: typeof body.version === "string" ? body.version : "", protocol: typeof body.protocol === "number" ? body.protocol : 1 },
          };
        } catch {
          return { url: endpoint.public_url, health: null };
        }
      }),
    );
    if (this.#auth !== auth || results.length === 0) return;
    const at = Date.now();
    for (const { url, health } of results) this.#list = markProbed(this.#list, url, at, health);
    this.#onChange();
  }

  #schedule(delay: number): void {
    this.disconnected();
    this.#probeTimer = setTimeout(() => {
      void this.probeOthers().finally(() => {
        if (this.#probeTimer === undefined) return;
        const interval = this.#probe.intervalMs ?? PROBE_INTERVAL_MS;
        // ±10% so tabs of several devices do not check in step.
        this.#schedule(interval * (0.9 + 0.2 * (this.#probe.random ?? Math.random)()));
      });
    }, delay);
  }
}
