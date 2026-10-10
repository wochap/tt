import { cbor, type PeerId } from "@automerge/automerge-repo/slim";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TicketWebSocketAdapter } from "./ticket-socket.ts";

/** The parts of WebSocket the adapter uses, driven by the test. */
class FakeSocket {
  static instances: FakeSocket[] = [];
  static readonly OPEN = 1;
  readyState = 0;
  binaryType = "blob";
  readonly listeners = new Map<string, ((event: unknown) => void)[]>();
  constructor(readonly url: string) {
    FakeSocket.instances.push(this);
  }
  addEventListener(type: string, listener: (event: unknown) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  fire(type: string, event: unknown = {}) {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
  send() {}
  close() {
    this.readyState = 3;
  }
  /** The server answers the join: the socket counts as connected. */
  accept() {
    this.readyState = 1;
    this.fire("open");
    const bytes: Uint8Array = cbor.encode({ type: "peer", senderId: "srv:a", targetId: "me", peerMetadata: {} });
    this.fire("message", { data: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) });
  }
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("ticket socket failover", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    FakeSocket.instances = [];
  });

  it("asks for the next member at once when onClose says so, and backs off otherwise", async () => {
    vi.stubGlobal("WebSocket", FakeSocket);
    let n = 0;
    const url = vi.fn(async () => `wss://member-${++n}/sync?ticket=t`);
    const backoff = vi.fn(() => 60_000);
    const onClose = vi.fn((opened: boolean) => opened);
    const states: string[] = [];
    const adapter = new TicketWebSocketAdapter({ url, backoff, onClose, onState: (state) => states.push(state) });
    adapter.connect("me" as PeerId);
    await flush();
    expect(FakeSocket.instances.map((socket) => socket.url)).toEqual(["wss://member-1/sync?ticket=t"]);
    FakeSocket.instances[0]!.accept();
    expect(adapter.connected).toBe(true);
    expect(states.at(-1)).toBe("open");

    // A connected member drops: the next one is asked for at once.
    FakeSocket.instances[0]!.fire("close");
    expect(onClose).toHaveBeenLastCalledWith(true);
    await flush();
    expect(url).toHaveBeenCalledTimes(2);
    expect(backoff).not.toHaveBeenCalled();
    expect(FakeSocket.instances[1]!.url).toBe("wss://member-2/sync?ticket=t");

    // That socket never connects and no member is left: back off.
    FakeSocket.instances[1]!.fire("close");
    expect(onClose).toHaveBeenLastCalledWith(false);
    await flush();
    expect(url).toHaveBeenCalledTimes(2);
    expect(backoff).toHaveBeenCalledWith(0);
    adapter.disconnect();
  });

  it("backs off when a full pass found no member", async () => {
    vi.stubGlobal("WebSocket", FakeSocket);
    const url = vi.fn(async () => {
      throw new Error("no member reachable");
    });
    const backoff = vi.fn(() => 60_000);
    const adapter = new TicketWebSocketAdapter({ url, backoff, onClose: () => true });
    adapter.connect("me" as PeerId);
    await flush();
    expect(url).toHaveBeenCalledTimes(1);
    expect(backoff).toHaveBeenCalledWith(0);
    expect(FakeSocket.instances).toHaveLength(0);
    adapter.disconnect();
  });
});
