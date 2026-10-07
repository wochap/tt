// Control messages between a tab and the repo worker. Repo traffic itself
// runs over a separate MessageChannel per tab (MessageChannelNetworkAdapter).

export interface Auth {
  /** Origin of tt-server, e.g. `https://tt.example.net` (no trailing slash). */
  server: string;
  token: string;
  indexDoc: string;
  user: { id: string; name: string };
}

export type SyncState =
  /** No auth: nothing to sync with. */
  | "signed-out"
  | "connecting"
  | "syncing"
  | "synced"
  | "offline"
  /** The server rejected the token (revoked); local data keeps working. */
  | "login-required";

export interface SyncStatus {
  state: SyncState;
  /** Local changes the server has not acknowledged, over every document. */
  pending: number;
  /** Last message from the server (ms since epoch). */
  lastSync: number | null;
  /** Shared worker, or a dedicated worker per tab (no SharedWorker support). */
  mode: "shared" | "dedicated";
}

export type ToWorker =
  | { t: "hello"; auth: Auth | null; repo: MessagePort }
  | { t: "auth"; auth: Auth | null }
  | { t: "reconnect" }
  | { t: "wipe"; id: number };

export type FromWorker =
  | { t: "status"; status: SyncStatus }
  | { t: "wiped"; id: number; error?: string };

export const DB_NAME = "tt";
export const DB_STORE = "documents";
export const META_DB = "tt-meta";
