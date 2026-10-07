// Tab side of the repo worker: a network-only Repo whose single peer is the
// worker (over a MessageChannel), plus the control port for auth, status and
// wipe.

import { Repo } from "@automerge/automerge-repo/slim";
import { MessageChannelNetworkAdapter } from "@automerge/automerge-repo-network-messagechannel";

import type { Auth, FromWorker, SyncStatus, ToWorker } from "./protocol.ts";
import { loadAutomerge } from "./wasm.ts";

interface ControlPort {
  postMessage(message: ToWorker, transfer?: Transferable[]): void;
  addEventListener(type: "message", listener: (event: MessageEvent<FromWorker>) => void): void;
}

function spawn(): { port: ControlPort; mode: SyncStatus["mode"] } {
  if (typeof SharedWorker !== "undefined") {
    const worker = new SharedWorker(new URL("./worker.ts", import.meta.url), { type: "module", name: "tt-repo" });
    worker.port.start();
    return { port: worker.port as ControlPort, mode: "shared" };
  }
  // Safari on iOS: one dedicated worker (and repo) per tab, same storage.
  const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module", name: "tt-repo" });
  return { port: worker as unknown as ControlPort, mode: "dedicated" };
}

export class SyncClient {
  readonly repo: Repo;
  readonly #port: ControlPort;
  #status: SyncStatus;
  readonly #listeners = new Set<() => void>();
  readonly #wipes = new Map<number, (error?: string) => void>();
  #nextWipe = 1;

  private constructor(port: ControlPort, mode: SyncStatus["mode"], auth: Auth | null) {
    this.#port = port;
    this.#status = { state: auth ? "connecting" : "signed-out", pending: 0, lastSync: null, mode };
    const channel = new MessageChannel();
    this.repo = new Repo({
      network: [new MessageChannelNetworkAdapter(channel.port1)],
      isEphemeral: true,
      sharePolicy: async () => true,
    });
    port.addEventListener("message", (event) => this.#receive(event.data));
    port.postMessage({ t: "hello", auth, repo: channel.port2 }, [channel.port2]);
    addEventListener("online", () => port.postMessage({ t: "reconnect" }));
  }

  static async start(auth: Auth | null): Promise<SyncClient> {
    await loadAutomerge();
    const { port, mode } = spawn();
    return new SyncClient(port, mode, auth);
  }

  #receive(message: FromWorker): void {
    if (message.t === "status") {
      this.#status = message.status;
      for (const listener of this.#listeners) listener();
    } else if (message.t === "wiped") {
      this.#wipes.get(message.id)?.(message.error);
      this.#wipes.delete(message.id);
    }
  }

  get status(): SyncStatus {
    return this.#status;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  getStatus = (): SyncStatus => this.#status;

  setAuth(auth: Auth | null): void {
    this.#port.postMessage({ t: "auth", auth });
  }

  /** Disconnects, deletes every local document and the sync metadata. */
  wipe(): Promise<void> {
    const id = this.#nextWipe++;
    return new Promise((resolve, reject) => {
      this.#wipes.set(id, (error) => (error ? reject(new Error(error)) : resolve()));
      this.#port.postMessage({ t: "wipe", id });
    });
  }
}
