// The session: server origin and identity, bearer token, index document,
// user, and the member endpoint list. Kept in localStorage ("stay signed in")
// or sessionStorage, never in a URL.

import type { Auth, Endpoint, ServerIdentity } from "./protocol.ts";

const KEY = "tt.auth";
const SERVER_KEY = "tt.server";

export class LoginError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = "LoginError";
    this.status = status;
  }
}

export function normalizeServer(input: string): string {
  const text = input.trim() || location.origin;
  const url = new URL(/^https?:\/\//i.test(text) ? text : `https://${text}`);
  return url.origin;
}

export function defaultServer(): string {
  return localStorage.getItem(SERVER_KEY) ?? location.origin;
}

export function loadAuth(): Auth | null {
  const raw = localStorage.getItem(KEY) ?? sessionStorage.getItem(KEY);
  if (!raw) return null;
  try {
    const auth = JSON.parse(raw) as Auth;
    return auth.token && auth.indexDoc && auth.server ? auth : null;
  } catch {
    return null;
  }
}

export function saveAuth(auth: Auth, persist: boolean): void {
  (persist ? localStorage : sessionStorage).setItem(KEY, JSON.stringify(auth));
  (persist ? sessionStorage : localStorage).removeItem(KEY);
  localStorage.setItem(SERVER_KEY, auth.server);
}

/** Rewrites the stored session in place, keeping its storage choice. */
export function updateAuth(auth: Auth): void {
  const storage = localStorage.getItem(KEY) ? localStorage : sessionStorage.getItem(KEY) ? sessionStorage : null;
  storage?.setItem(KEY, JSON.stringify(auth));
}

/** Stores the worker's refreshed endpoint list with the session it belongs to (dropped if signed out since). */
export function saveEndpoints(token: string, endpoints: Endpoint[]): void {
  const current = loadAuth();
  if (current?.token === token) updateAuth({ ...current, endpoints });
}

/** Signs out locally: the session and its endpoint list go. */
export function clearAuth(): void {
  localStorage.removeItem(KEY);
  sessionStorage.removeItem(KEY);
}

/** Subscribes to sign-in/out in other tabs. */
export function onAuthChange(listener: () => void): () => void {
  const handler = (event: StorageEvent) => {
    if (event.key === KEY || event.key === null) listener();
  };
  addEventListener("storage", handler);
  return () => removeEventListener("storage", handler);
}

export async function login(server: string, username: string, password: string): Promise<Auth> {
  let response: Response;
  try {
    response = await fetch(`${server}/api/login`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
  } catch {
    throw new LoginError(0, `Cannot reach ${server}`);
  }
  const body = (await response.json().catch(() => ({}))) as {
    token?: string;
    index_doc?: string;
    user?: { id: string; name: string };
    server?: ServerIdentity;
    error?: string;
  };
  if (!response.ok || !body.token || !body.index_doc || !body.user) {
    const message =
      response.status === 401
        ? "Invalid username or password"
        : response.status === 409 && body.error === "account_conflict"
          ? ACCOUNT_CONFLICT
          : response.status === 429
          ? "Too many attempts; wait a minute"
          : (body.error ?? `Sign-in failed (HTTP ${response.status})`);
    throw new LoginError(response.status, message);
  }
  return { server, token: body.token, indexDoc: body.index_doc, user: body.user, identity: identity(body.server) };
}

export const ACCOUNT_CONFLICT = "This account name is also used on another server. Ask the admin to rename it.";

function identity(value: ServerIdentity | null | undefined): ServerIdentity | undefined {
  return value?.id && value.name ? { id: value.id, name: value.name } : undefined;
}

/** The session with user and server identity from `/api/me`, or null when offline or rejected. */
export async function refreshMe(auth: Auth): Promise<Auth | null> {
  try {
    const response = await fetch(`${auth.server}/api/me`, { headers: { authorization: `Bearer ${auth.token}` }, cache: "no-store" });
    if (!response.ok) return null;
    const body = (await response.json()) as { user?: { id: string; name: string }; server?: ServerIdentity };
    return { ...auth, user: body.user ?? auth.user, identity: identity(body.server) ?? auth.identity };
  } catch {
    return null;
  }
}

/** The server's identity from `/api/health` (no session needed); null if unknown or unreachable. */
export async function fetchIdentity(server: string, signal?: AbortSignal): Promise<ServerIdentity | null> {
  try {
    const response = await fetch(`${server}/api/health`, { signal, cache: "no-store" });
    const body = (await response.json()) as { server?: ServerIdentity | null };
    return identity(body.server) ?? null;
  } catch {
    return null;
  }
}

/**
 * Revokes the token through the first member that answers (any member
 * revokes it on all of them); offline logout still proceeds locally.
 */
export async function revoke(auth: Auth): Promise<void> {
  const servers = [...new Set([auth.server, ...(auth.endpoints ?? []).map((endpoint) => endpoint.public_url)])];
  for (const server of servers) {
    try {
      await fetch(`${server}/api/logout`, {
        method: "POST",
        headers: { authorization: `Bearer ${auth.token}` },
      });
      return;
    } catch {
      // unreachable: try the next member
    }
  }
  // offline: the token stays valid until revoked on a server
}

export async function reachable(server: string, signal?: AbortSignal): Promise<boolean> {
  try {
    const response = await fetch(`${server}/api/health`, { signal, cache: "no-store" });
    return response.ok;
  } catch {
    return false;
  }
}
