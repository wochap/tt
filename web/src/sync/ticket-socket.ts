// A websocket network adapter for tt-server's `/sync` that fetches a fresh
// single-use ticket before every connection (the stock adapter reuses one
// URL forever) and reconnects with exponential backoff capped at 30 s. The
// URL provider may pick another member each time (see endpoints.ts); after a
// close, `onClose` decides whether the next attempt starts at once.

import { cbor, type Message, NetworkAdapter, type PeerId, type PeerMetadata } from "@automerge/automerge-repo/slim";

export class Unauthorized extends Error {
  constructor() {
    super("token rejected");
    this.name = "Unauthorized";
  }
}

export type SocketState = "connecting" | "open" | "closed" | "unauthorized";

export interface TicketSocketOptions {
  /** Resolves to a `wss://…/sync?ticket=…` URL; throws `Unauthorized` on 401. */
  url: () => Promise<string>;
  /** The socket closed (`opened`: it had connected); true retries at once instead of after the backoff. */
  onClose?: (opened: boolean) => boolean;
  onState?: (state: SocketState) => void;
  /** Every repo message received from the server. */
  onInbound?: (message: Message) => void;
  /** Every repo message sent to the server. */
  onOutbound?: (message: Message) => void;
  /** Delay before the n-th retry (n ≥ 0); default 1 s doubling to 30 s with jitter. */
  backoff?: (attempt: number) => number;
}

export const MAX_BACKOFF_MS = 30_000;

export function defaultBackoff(attempt: number): number {
  const base = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** attempt);
  return Math.round(base * (0.8 + Math.random() * 0.2));
}

const PROTOCOL_V1 = "1";

export class TicketWebSocketAdapter extends NetworkAdapter {
  readonly #options: TicketSocketOptions;
  #socket: WebSocket | undefined;
  #remotePeerId: PeerId | undefined;
  #attempt = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #stopped = false;
  #opening = false;
  #ready = false;
  #resolveReady!: () => void;
  readonly #readyPromise = new Promise<void>((resolve) => (this.#resolveReady = resolve));

  constructor(options: TicketSocketOptions) {
    super();
    this.#options = options;
  }

  isReady(): boolean {
    return this.#ready;
  }

  whenReady(): Promise<void> {
    return this.#readyPromise;
  }

  #markReady(): void {
    if (this.#ready) return;
    this.#ready = true;
    this.#resolveReady();
  }

  get connected(): boolean {
    return this.#socket?.readyState === WebSocket.OPEN && this.#remotePeerId !== undefined;
  }

  connect(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    this.peerId = peerId;
    this.peerMetadata = peerMetadata ?? {};
    // Do not hold up "unavailable" answers for more than a second offline.
    setTimeout(() => this.#markReady(), 1000);
    void this.#open();
  }

  /** Skips the remaining backoff (e.g. on the browser's `online` event). */
  reconnectNow(): void {
    if (this.#stopped || this.#opening || this.#socket) return;
    clearTimeout(this.#timer);
    this.#timer = undefined;
    void this.#open();
  }

  async #open(): Promise<void> {
    if (this.#stopped || this.#opening) return;
    this.#opening = true;
    this.#options.onState?.("connecting");
    let url: string;
    try {
      url = await this.#options.url();
    } catch (error) {
      this.#opening = false;
      if (error instanceof Unauthorized) {
        this.#options.onState?.("unauthorized");
        this.#markReady();
        return;
      }
      this.#options.onState?.("closed");
      this.#scheduleRetry();
      return;
    }
    this.#opening = false;
    if (this.#stopped) return;
    const socket = new WebSocket(url);
    socket.binaryType = "arraybuffer";
    this.#socket = socket;
    socket.addEventListener("open", () => {
      this.#send({
        type: "join",
        senderId: this.peerId!,
        peerMetadata: this.peerMetadata ?? {},
        supportedProtocolVersions: [PROTOCOL_V1],
      });
    });
    socket.addEventListener("message", (event) => this.#receive(event.data as ArrayBuffer));
    socket.addEventListener("close", () => {
      if (this.#socket !== socket) return;
      this.#socket = undefined;
      const opened = this.#remotePeerId !== undefined;
      if (this.#remotePeerId) this.emit("peer-disconnected", { peerId: this.#remotePeerId });
      this.#remotePeerId = undefined;
      this.#options.onState?.("closed");
      if (!this.#stopped && this.#options.onClose?.(opened)) void this.#open();
      else this.#scheduleRetry();
    });
  }

  #scheduleRetry(): void {
    if (this.#stopped || this.#timer) return;
    const delay = (this.#options.backoff ?? defaultBackoff)(this.#attempt++);
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      void this.#open();
    }, delay);
  }

  #receive(data: ArrayBuffer): void {
    if (!data.byteLength) return;
    let message: Message & { peerMetadata?: PeerMetadata; message?: string };
    try {
      message = cbor.decode(new Uint8Array(data));
    } catch {
      return;
    }
    if (message.type === "peer") {
      this.#attempt = 0;
      this.#remotePeerId = message.senderId;
      this.#markReady();
      this.#options.onState?.("open");
      this.emit("peer-candidate", { peerId: message.senderId, peerMetadata: message.peerMetadata ?? {} });
      return;
    }
    if (message.type === "error") {
      console.warn("tt sync: server error", message.message);
      return;
    }
    this.#options.onInbound?.(message);
    this.emit("message", message);
  }

  #send(message: unknown): void {
    const socket = this.#socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    const bytes: Uint8Array = cbor.encode(message);
    socket.send(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer);
  }

  send(message: Message): void {
    if ("data" in message && message.data?.byteLength === 0) return;
    // Messages for a closed socket are dropped; the repo resyncs on reconnect.
    if (!this.connected) return;
    this.#options.onOutbound?.(message);
    this.#send(message);
  }

  disconnect(): void {
    this.#stopped = true;
    clearTimeout(this.#timer);
    const socket = this.#socket;
    this.#socket = undefined;
    socket?.close();
    if (this.#remotePeerId) this.emit("peer-disconnected", { peerId: this.#remotePeerId });
    this.#remotePeerId = undefined;
    this.#options.onState?.("closed");
  }
}
