// The session: server origin, bearer token, index document and user. Kept in
// localStorage ("stay signed in") or sessionStorage, never in a URL.

import type { Auth } from "./protocol.ts";

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
    error?: string;
  };
  if (!response.ok || !body.token || !body.index_doc || !body.user) {
    const message =
      response.status === 401
        ? "Invalid username or password"
        : response.status === 429
          ? "Too many attempts; wait a minute"
          : (body.error ?? `Sign-in failed (HTTP ${response.status})`);
    throw new LoginError(response.status, message);
  }
  return { server, token: body.token, indexDoc: body.index_doc, user: body.user };
}

/** Revokes the token on the server; offline logout still proceeds locally. */
export async function revoke(auth: Auth): Promise<void> {
  try {
    await fetch(`${auth.server}/api/logout`, {
      method: "POST",
      headers: { authorization: `Bearer ${auth.token}` },
    });
  } catch {
    // offline: the token stays valid until revoked on the server
  }
}

export async function reachable(server: string, signal?: AbortSignal): Promise<boolean> {
  try {
    const response = await fetch(`${server}/api/health`, { signal, cache: "no-store" });
    return response.ok;
  } catch {
    return false;
  }
}
