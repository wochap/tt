// Control messages between a tab and the repo worker. Repo traffic itself
// runs over a separate MessageChannel per tab (MessageChannelNetworkAdapter).

/** The server's own identity; `id` is 26 base32 characters, shown as 8. */
export interface ServerIdentity {
  id: string;
  name: string;
}

/** Client protocol versions this bundle speaks (`protocol` in `/api/health`). */
export const SUPPORTED_PROTOCOLS = { min: 2, max: 2 } as const;

/** One member server the worker may sync with (from `/api/peers`). */
export interface Endpoint {
  /** Empty until the member's id is known (the sign-in server before its first connection). */
  server_id: string;
  name: string;
  /** Origin of the member's client URL, e.g. `https://laptop-b.example.ts.net`. */
  public_url: string;
  /** Last successful connection (ms since epoch); null if never. */
  last_ok: number | null;
  /** The member's last health check reported a protocol outside `SUPPORTED_PROTOCOLS`. */
  incompatible?: boolean;
}

export interface Auth {
  /** Origin of the tt-server signed in to, e.g. `https://tt.example.net` (no trailing slash). */
  server: string;
  token: string;
  indexDoc: string;
  user: { id: string; name: string };
  /** From login, refreshed by `/api/me`; missing in sessions from older builds. */
  identity?: ServerIdentity;
  /** Every member the worker may sync with, kept fresh by the worker; missing until the first connection. */
  endpoints?: Endpoint[];
}

/** The server's name for labels, or its host before the name is known. */
export function serverName(auth: Pick<Auth, "server" | "identity">): string {
  return auth.identity?.name ?? new URL(auth.server).host;
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
  /** The member the worker is connected to; null or missing when not connected. */
  member?: Pick<Endpoint, "server_id" | "name" | "public_url"> | null;
  /** Every known member, for Settings. */
  members?: Endpoint[];
}

export type ToWorker =
  | { t: "hello"; auth: Auth | null; repo: MessagePort }
  | { t: "auth"; auth: Auth | null }
  | { t: "reconnect" }
  | { t: "wipe"; id: number };

export type FromWorker =
  | { t: "status"; status: SyncStatus }
  /** The refreshed endpoint list of the session with `token`, for the tab to persist. */
  | { t: "endpoints"; token: string; endpoints: Endpoint[] }
  | { t: "wiped"; id: number; error?: string };

export const DB_NAME = "tt";
export const DB_STORE = "documents";
export const META_DB = "tt-meta";
