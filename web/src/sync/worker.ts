// The repo worker: one Automerge repo per browser profile, persisted in
// IndexedDB. Every tab is a MessageChannel peer of this repo; only the worker
// talks to tt-server (`/sync`, with a fresh ticket per connection). It syncs
// with one member of the root at a time, failing over between members (see
// endpoints.ts); all of them sync into the same repo. Runs as a SharedWorker,
// or as a dedicated worker per tab where SharedWorker is missing.

import { type Message, Repo } from "@automerge/automerge-repo/slim";
import { MessageChannelNetworkAdapter } from "@automerge/automerge-repo-network-messagechannel";
import { IndexedDBStorageAdapter } from "@automerge/automerge-repo-storage-indexeddb";

import { MemberEndpoints } from "./member-endpoints.ts";
import { PendingTracker } from "./pending.ts";
import { type Auth, DB_NAME, DB_STORE, type FromWorker, META_DB, type SyncStatus, type ToWorker } from "./protocol.ts";
import { type SocketState, TicketWebSocketAdapter } from "./ticket-socket.ts";
import { loadAutomerge } from "./wasm.ts";

/** The parts of Shared/DedicatedWorkerGlobalScope used here (the project compiles against the DOM lib). */
interface WorkerScope {
  addEventListener(type: string, listener: (event: MessageEvent) => void): void;
  postMessage(message: unknown): void;
}
type Port = MessagePort | WorkerScope;

const scope = self as unknown as WorkerScope;
const shared = "onconnect" in self;
const ports = new Set<Port>();

let storage: IndexedDBStorageAdapter;
let repo: Repo;
let pending: PendingTracker;
let auth: Auth | null = null;
let socket: TicketWebSocketAdapter | undefined;
const members = new MemberEndpoints((token, endpoints) => {
  broadcast({ t: "endpoints", token, endpoints });
  publish(true);
});
let socketState: SocketState = "closed";
let lastSync: number | null = null;
let status: SyncStatus = { state: "signed-out", pending: 0, lastSync: null, mode: shared ? "shared" : "dedicated" };

const booted = (async () => {
  await loadAutomerge();
  openRepo();
})();

function openRepo(): void {
  storage = new IndexedDBStorageAdapter(DB_NAME, DB_STORE);
  repo = new Repo({ storage, network: [], sharePolicy: async () => true });
  pending = new PendingTracker(repo, META_DB, () => publish());
}

function broadcast(message: FromWorker): void {
  for (const port of ports) port.postMessage(message);
}

function computeStatus(): SyncStatus {
  const count = pending.count();
  let state: SyncStatus["state"];
  if (!auth) state = "signed-out";
  else if (socketState === "unauthorized") state = "login-required";
  else if (socket?.connected) state = count > 0 ? "syncing" : "synced";
  else if (socketState === "connecting" && lastSync === null) state = "connecting";
  else state = "offline";
  const current = socket?.connected ? members.current : null;
  return {
    state,
    pending: count,
    lastSync,
    mode: status.mode,
    member: current && { server_id: current.server_id, name: current.name, public_url: current.public_url },
    members: members.list,
    switchedAt: members.switchedAt,
  };
}

let publishTimer: ReturnType<typeof setTimeout> | undefined;
function publish(now = false): void {
  const run = () => {
    publishTimer = undefined;
    const next = computeStatus();
    if (JSON.stringify(next) !== JSON.stringify(status)) {
      status = next;
      broadcast({ t: "status", status });
    }
  };
  if (now) {
    clearTimeout(publishTimer);
    run();
  } else {
    publishTimer ??= setTimeout(run, 250);
  }
}

function connect(next: Auth | null): void {
  const same = auth && next && auth.server === next.server && auth.token === next.token;
  auth = next;
  if (same && socketState !== "unauthorized") {
    publish(true);
    return;
  }
  if (socket) {
    repo.networkSubsystem.removeNetworkAdapter(socket);
    socket.disconnect();
    socket = undefined;
  }
  socketState = "closed";
  lastSync = null;
  const rotation = members.reset(next, () => publish(true));
  if (next && rotation) {
    socket = new TicketWebSocketAdapter({
      url: () => rotation.url(),
      onClose: (opened) => rotation.closed(opened),
      onState: (state) => {
        socketState = state;
        if (state === "open") void members.connected(next);
        else members.disconnected();
        publish(true);
      },
      onInbound: (message: Message) => {
        lastSync = Date.now();
        pending.observeServer(message);
        publish();
      },
    });
    repo.networkSubsystem.addNetworkAdapter(socket);
  }
  publish(true);
}

async function wipe(): Promise<void> {
  connect(null);
  await repo.shutdown().catch(() => undefined);
  // `dbPromise` is private in the typings; the open connection blocks deletion.
  (await (storage as unknown as { dbPromise: Promise<IDBDatabase> }).dbPromise).close();
  await pending.close();
  await Promise.all([deleteDatabase(DB_NAME), deleteDatabase(META_DB)]);
  openRepo();
  // Tabs reconnect their repo ports after a wipe (they reload).
  publish(true);
}

function deleteDatabase(name: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.deleteDatabase(name);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
    request.onblocked = () => console.warn(`tt: deleting ${name} is blocked by an open connection`);
  });
}

function attach(port: Port): void {
  ports.add(port);
  port.addEventListener("message", (event: MessageEvent<ToWorker>) => {
    void booted.then(() => handle(port, event.data));
  });
  if ("start" in port) port.start();
}

async function handle(port: Port, message: ToWorker): Promise<void> {
  switch (message.t) {
    case "hello": {
      // Weak refs let the worker drop adapters of closed tabs.
      repo.networkSubsystem.addNetworkAdapter(new MessageChannelNetworkAdapter(message.repo, { useWeakRef: true }));
      if (message.auth && !auth) connect(message.auth);
      else if (!message.auth && auth && !shared) connect(null);
      port.postMessage({ t: "status", status: computeStatus() } satisfies FromWorker);
      break;
    }
    case "auth":
      connect(message.auth);
      break;
    case "reconnect":
      socket?.reconnectNow();
      break;
    case "wipe":
      try {
        await wipe();
        broadcast({ t: "wiped", id: message.id });
      } catch (error) {
        broadcast({ t: "wiped", id: message.id, error: String(error) });
      }
      break;
  }
}

if (shared) {
  scope.addEventListener("connect", (event) => {
    const port = event.ports[0];
    if (port) attach(port);
  });
} else {
  attach(scope);
}

scope.addEventListener("online", () => socket?.reconnectNow());
