import { afterEach, describe, expect, it, vi } from "vitest";

import { clearAuth, loadAuth, saveAuth, saveEndpoints } from "./auth.ts";
import { MemberRotation, NoMemberReachable, orderEndpoints, refreshEndpoints, seedEndpoints } from "./endpoints.ts";
import { MemberEndpoints } from "./member-endpoints.ts";
import type { Auth, Endpoint } from "./protocol.ts";
import type { Peer, Peers } from "./server-api.ts";
import { Unauthorized } from "./ticket-socket.ts";

const A = "https://laptop-a.example.ts.net";
const B = "https://laptop-b.example.ts.net";
const C = "https://laptop-c.example.ts.net";
const ID = { a: "aaaaaaaaaaaaaaaaaaaaaaaaaa", b: "bbbbbbbbbbbbbbbbbbbbbbbbbb", c: "cccccccccccccccccccccccccc" };

const AUTH: Auth = {
  server: A,
  token: "tt2.token",
  indexDoc: "i",
  user: { id: "u", name: "wochap" },
  identity: { id: ID.a, name: "laptop-a" },
};

function endpoint(url: string, over: Partial<Endpoint> = {}): Endpoint {
  const name = new URL(url).host.split(".")[0]!;
  return { server_id: ID[name.slice(-1) as "a" | "b" | "c"], name, public_url: url, last_ok: null, ...over };
}

function peer(id: string, name: string, public_url: string | null): Peer {
  return { id, name, state: "online", pending: 0, last_seen: null, address: null, public_url, error: null };
}

function peersOf(server: { id: string; name: string }, peers: Peer[]): Peers {
  return { server, peers };
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

type MemberState = "up" | "down" | "hang" | "old" | "revoked-token";

/**
 * Fake members by origin: `up` answers health (protocol 2), tickets and
 * peers; `down` refuses connections; `hang` never answers (until aborted);
 * `old` reports protocol 1; `revoked-token` answers 401 to the token.
 */
function fakeMembers(states: Record<string, MemberState>, peers: Record<string, Peers> = {}) {
  const calls: string[] = [];
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input));
    calls.push(`${url.origin}${url.pathname}`);
    const state = states[url.origin] ?? "down";
    if (state === "down") throw new TypeError("Failed to fetch");
    if (state === "hang") {
      return new Promise<Response>((_, reject) => {
        init?.signal?.addEventListener("abort", () => reject(new DOMException("timed out", "TimeoutError")));
      });
    }
    if (url.pathname === "/api/health") return json(200, { ok: true, protocol: state === "old" ? 1 : 2, public_url: url.origin });
    if (new Headers(init?.headers).get("authorization") !== `Bearer ${AUTH.token}`) return json(401, {});
    if (state === "revoked-token") return json(401, { error: "unauthorized" });
    if (url.pathname === "/api/ws-ticket") return json(200, { ticket: `ticket-from-${url.hostname}`, expires_in: 60 });
    if (url.pathname === "/api/peers") return json(200, peers[url.origin] ?? { server: { id: "", name: null }, peers: [] });
    return json(404, {});
  });
  return { fetch, calls };
}

describe("endpoint list", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
    sessionStorage.clear();
  });

  it("is seeded with the sign-in server", () => {
    expect(seedEndpoints(AUTH)).toEqual([{ server_id: ID.a, name: "laptop-a", public_url: A, last_ok: null }]);
    // A stored list keeps the sign-in server even when it lost it.
    expect(seedEndpoints({ ...AUTH, endpoints: [endpoint(B)] }).map((e) => e.public_url)).toEqual([B, A]);
  });

  it("learns every member with a client URL after login, keeping success times", () => {
    const seed = [{ ...endpoint(A), last_ok: 5 }];
    const list = refreshEndpoints(
      seed,
      peersOf({ id: ID.a, name: "laptop-a" }, [peer(ID.b, "laptop-b", `${B}/`), peer(ID.c, "laptop-c", null)]),
      seed[0]!,
      A,
    );
    expect(list).toEqual([
      { server_id: ID.a, name: "laptop-a", public_url: A, last_ok: 5 },
      { server_id: ID.b, name: "laptop-b", public_url: B, last_ok: null },
    ]);
  });

  it("drops a revoked member on the next refresh but always keeps the sign-in server", () => {
    const list = [endpoint(A, { last_ok: 1 }), endpoint(B, { last_ok: 9 })];
    // Connected to B after A was revoked: A is the sign-in server, so it stays.
    const fromB = refreshEndpoints(list, peersOf({ id: ID.b, name: "laptop-b" }, []), list[1]!, A);
    expect(fromB.map((e) => e.public_url)).toEqual([B, A]);
    // Connected to A after B was revoked: B is gone.
    const fromA = refreshEndpoints(list, peersOf({ id: ID.a, name: "laptop-a" }, [peer(ID.c, "laptop-c", C)]), list[0]!, A);
    expect(fromA.map((e) => e.public_url)).toEqual([A, C]);
  });

  it("refreshes after a successful connection and hands the list to the tabs", async () => {
    const members = fakeMembers(
      { [A]: "up", [B]: "up" },
      { [A]: peersOf({ id: ID.a, name: "laptop-a" }, [peer(ID.b, "laptop-b", B)]) },
    );
    vi.stubGlobal("fetch", members.fetch);
    const persisted: Endpoint[][] = [];
    const session = new MemberEndpoints((token, endpoints) => {
      expect(token).toBe(AUTH.token);
      persisted.push(endpoints);
    });
    const rotation = session.reset(AUTH, () => {})!;
    expect(await rotation.url()).toBe("wss://laptop-a.example.ts.net/sync?ticket=ticket-from-laptop-a.example.ts.net");
    await session.connected(AUTH);
    expect(session.current?.public_url).toBe(A);
    expect(session.list.map((e) => e.public_url)).toEqual([A, B]);
    expect(session.list[0]!.last_ok).toEqual(expect.any(Number));
    expect(persisted).toEqual([session.list]);
    vi.unstubAllGlobals();
  });

  it("is wiped on sign-out", () => {
    saveAuth(AUTH, true);
    saveEndpoints(AUTH.token, [endpoint(A), endpoint(B)]);
    expect(loadAuth()?.endpoints).toHaveLength(2);
    // A list of an older session is not stored with the new one.
    saveEndpoints("tt2.other", [endpoint(C)]);
    expect(loadAuth()?.endpoints).toHaveLength(2);
    clearAuth();
    expect(loadAuth()).toBeNull();
    saveEndpoints(AUTH.token, [endpoint(A)]);
    expect(loadAuth()).toBeNull();
    // The worker drops its copy too.
    const session = new MemberEndpoints(() => {});
    session.reset({ ...AUTH, endpoints: [endpoint(A), endpoint(B)] }, () => {});
    expect(session.list).toHaveLength(2);
    expect(session.reset(null, () => {})).toBeUndefined();
    expect(session.list).toEqual([]);
    expect(session.current).toBeNull();
  });
});

describe("member rotation", () => {
  const rotation = (list: Endpoint[], members: ReturnType<typeof fakeMembers>, onCompatibility = vi.fn()) =>
    new MemberRotation({ endpoints: () => list, token: () => AUTH.token, fetch: members.fetch, timeoutMs: 50, onCompatibility });

  it("orders by last success, never-reached members last", () => {
    const list = [endpoint(A), endpoint(B, { last_ok: 10 }), endpoint(C, { last_ok: 20 })];
    expect(orderEndpoints(list).map((e) => e.name)).toEqual(["laptop-c", "laptop-b", "laptop-a"]);
  });

  it("origin down: takes the ticket from the next member and connects there", async () => {
    const members = fakeMembers({ [A]: "down", [B]: "up" });
    const list = [endpoint(A, { last_ok: 20 }), endpoint(B, { last_ok: 10 })];
    const picker = rotation(list, members);
    expect(await picker.url()).toBe("wss://laptop-b.example.ts.net/sync?ticket=ticket-from-laptop-b.example.ts.net");
    expect(picker.current?.public_url).toBe(B);
    expect(members.calls).toEqual([`${A}/api/health`, `${B}/api/health`, `${B}/api/ws-ticket`]);
  });

  it("an unanswered health check times out and the next member is tried", async () => {
    const members = fakeMembers({ [A]: "hang", [B]: "up" });
    const started = Date.now();
    expect(await rotation([endpoint(A), endpoint(B)], members).url()).toContain("laptop-b");
    expect(Date.now() - started).toBeLessThan(1000);
  });

  it("all down: a full pass fails, then the next call starts a new pass", async () => {
    const members = fakeMembers({ [A]: "down", [B]: "down", [C]: "down" });
    const picker = rotation([endpoint(A), endpoint(B), endpoint(C)], members);
    await expect(picker.url()).rejects.toBeInstanceOf(NoMemberReachable);
    expect(members.calls).toHaveLength(3);
    await expect(picker.url()).rejects.toBeInstanceOf(NoMemberReachable);
    expect(members.calls).toHaveLength(6);
  });

  it("incompatible member: skipped and marked, another member is used", async () => {
    const members = fakeMembers({ [A]: "up", [B]: "old" });
    const onCompatibility = vi.fn();
    const list = [endpoint(B, { last_ok: 20 }), endpoint(A, { last_ok: 10 })];
    expect(await rotation(list, members, onCompatibility).url()).toContain("laptop-a");
    expect(onCompatibility).toHaveBeenCalledWith(list[0], true, undefined);
    expect(members.calls).not.toContain(`${B}/api/ws-ticket`);
  });

  it("a member that is compatible again is unmarked", async () => {
    const members = fakeMembers({ [B]: "up" });
    const onCompatibility = vi.fn();
    const list = [endpoint(B, { incompatible: true })];
    await rotation(list, members, onCompatibility).url();
    expect(onCompatibility).toHaveBeenCalledWith(list[0], false, undefined);
  });

  it("after a drop the next member is tried at once, the dropped one only in the next pass", async () => {
    const members = fakeMembers({ [A]: "up", [B]: "up" });
    const list = [endpoint(A, { last_ok: 20 }), endpoint(B, { last_ok: 10 })];
    const picker = rotation(list, members);
    expect(await picker.url()).toContain("laptop-a");
    picker.connected();
    // A drops: B is left in this pass, so retry at once.
    expect(picker.closed(true)).toBe(true);
    expect(await picker.url()).toContain("laptop-b");
    // B's socket fails before connecting: nothing left, back off; the next pass starts with A again.
    expect(picker.closed(false)).toBe(false);
    expect(await picker.url()).toContain("laptop-a");
  });

  it("with a single member a drop backs off", async () => {
    const members = fakeMembers({ [A]: "up" });
    const picker = rotation([endpoint(A)], members);
    await picker.url();
    picker.connected();
    expect(picker.closed(true)).toBe(false);
    expect(await picker.url()).toContain("laptop-a");
  });

  it("every reachable member refusing the token means it was revoked", async () => {
    const members = fakeMembers({ [A]: "revoked-token", [B]: "down" });
    await expect(rotation([endpoint(A), endpoint(B)], members).url()).rejects.toBeInstanceOf(Unauthorized);
  });
});

describe("health checks of members not in use", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  /** Members by origin: a version answers health with that version, "old" with protocol 1, "down" refuses. */
  function healthOnly(states: Record<string, string>) {
    const calls: string[] = [];
    const fetch = vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input));
      calls.push(`${url.origin}${url.pathname}`);
      const state = states[url.origin] ?? "down";
      if (state === "down") throw new TypeError("Failed to fetch");
      if (url.pathname === "/api/health") return json(200, state === "old" ? { version: "1.2.0", protocol: 1 } : { version: state, protocol: 2 });
      if (url.pathname === "/api/ws-ticket") return json(200, { ticket: "t" });
      if (url.pathname === "/api/peers") return json(200, { server: { id: ID[url.hostname.split(".")[0]!.slice(-1) as "a"], name: url.hostname.split(".")[0] }, peers: [peer(ID.a, "laptop-a", A), peer(ID.b, "laptop-b", B), peer(ID.c, "laptop-c", C)] });
      return json(404, {});
    });
    return { fetch, calls };
  }

  async function connectedTo(states: Record<string, string>, list: Endpoint[], online = true) {
    const members = healthOnly(states);
    vi.stubGlobal("fetch", members.fetch);
    const onChange = vi.fn();
    const session = new MemberEndpoints(() => {}, { fetch: members.fetch, online: () => online, random: () => 0.5, timeoutMs: 50 });
    const auth = { ...AUTH, endpoints: list };
    const rotation = session.reset(auth, onChange)!;
    await rotation.url();
    session.disconnected();
    return { session, auth, members, onChange };
  }

  it("records reachable, unreachable and incompatible members with their version", async () => {
    const list = [endpoint(A, { last_ok: 9 }), endpoint(B), endpoint(C)];
    const { session, members, onChange } = await connectedTo({ [A]: "0.9.0", [B]: "0.9.1", [C]: "old" }, list);
    members.calls.length = 0;
    await session.probeOthers();
    // Only members not in use, and only their health: no ticket, no socket.
    expect(members.calls.sort()).toEqual([`${B}/api/health`, `${C}/api/health`]);
    const [a, b, c] = session.list;
    expect(a!.reachable).toBeUndefined();
    expect(b).toMatchObject({ reachable: true, version: "0.9.1", protocol: 2, checked_at: expect.any(Number) });
    expect(b!.incompatible).toBeUndefined();
    expect(c).toMatchObject({ reachable: true, version: "1.2.0", protocol: 1, incompatible: true });
    expect(onChange).toHaveBeenCalled();
  });

  it("a member that stops answering is unreachable", async () => {
    const list = [endpoint(A, { last_ok: 9 }), endpoint(B, { reachable: true, version: "0.9.1" })];
    const { session } = await connectedTo({ [A]: "0.9.0" }, list);
    await session.probeOthers();
    expect(session.list[1]).toMatchObject({ reachable: false, checked_at: expect.any(Number) });
  });

  it("does nothing while offline", async () => {
    const { session, members } = await connectedTo({ [A]: "0.9.0", [B]: "0.9.1" }, [endpoint(A, { last_ok: 9 }), endpoint(B)], false);
    members.calls.length = 0;
    await session.probeOthers();
    expect(members.calls).toEqual([]);
  });

  it("runs once after connecting, then about once a minute until disconnected", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const { session, auth, members } = await connectedTo({ [A]: "0.9.0", [B]: "0.9.1" }, [endpoint(A, { last_ok: 9 }), endpoint(B)]);
    await session.connected(auth);
    const probes = () => members.calls.filter((call) => call === `${B}/api/health`).length;
    await vi.advanceTimersByTimeAsync(0);
    expect(probes()).toBe(1);
    await vi.advanceTimersByTimeAsync(59_000);
    expect(probes()).toBe(1);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(probes()).toBe(2);
    session.disconnected();
    await vi.advanceTimersByTimeAsync(180_000);
    expect(probes()).toBe(2);
  });

  it("the first connection is no switch; a failover is", async () => {
    const list = [endpoint(A, { last_ok: 9 }), endpoint(B)];
    const { session, auth } = await connectedTo({ [A]: "0.9.0", [B]: "0.9.1" }, list);
    const rotation = session.reset(auth, () => {})!;
    await rotation.url();
    await session.connected(auth);
    session.disconnected();
    expect(session.switchedAt).toBeNull();
    // A drops; B is used next.
    rotation.closed(true);
    expect(await rotation.url()).toContain("laptop-b");
    await session.connected(auth);
    session.disconnected();
    expect(session.switchedAt).toEqual(expect.any(Number));
    // A new session starts without a switch.
    session.reset(auth, () => {});
    expect(session.switchedAt).toBeNull();
  });
});
