import { type FormEvent, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Checkbox } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { ACCOUNT_CONFLICT, defaultServer, fetchIdentity, LoginError, login, normalizeServer } from "@/sync/auth";
import type { Auth } from "@/sync/protocol";

function origin(server: string): string | undefined {
  try {
    return normalizeServer(server);
  } catch {
    return undefined;
  }
}

/**
 * Frames 6 and 5.10: "Sign in to <server name>", works offline after that. An
 * account conflict (409) is a banner, not the wrong-password error: retrying
 * cannot fix it, so Sign in stays disabled until the username changes.
 */
export function LoginPage({ onLogin }: { onLogin: (auth: Auth, persist: boolean) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [show, setShow] = useState(false);
  const [persist, setPersist] = useState(true);
  const [server, setServer] = useState(defaultServer);
  const [editServer, setEditServer] = useState(false);
  const [error, setError] = useState<string>();
  const [conflict, setConflict] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState<string>();
  const url = origin(server);

  // The server's name from /api/health; its host until known or when unreachable.
  useEffect(() => {
    setName(undefined);
    if (!url) return;
    const controller = new AbortController();
    const timer = setTimeout(() => void fetchIdentity(url, controller.signal).then((id) => id && setName(id.name)), 250);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [url]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setError(undefined);
    setConflict(undefined);
    let origin: string;
    try {
      origin = normalizeServer(server);
    } catch {
      setError("Server must be a URL such as https://tt.example.net");
      return;
    }
    setBusy(true);
    try {
      onLogin(await login(origin, username.trim(), password), persist);
    } catch (e) {
      if (e instanceof LoginError && e.message === ACCOUNT_CONFLICT) setConflict(username.trim());
      else setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  const blocked = conflict !== undefined && conflict === username.trim();

  return (
    <div className="flex min-h-full items-center justify-center bg-base text-fg">
      <form onSubmit={submit} className="flex w-full max-w-[560px] flex-col items-start gap-[22px] px-6 py-10 sm:px-[72px]" aria-label="Sign in">
        <div>
          <div className="text-[26px] font-semibold tracking-[-.03em] text-link">tt</div>
          <h1 className="mt-[14px] text-[20px] font-medium tracking-[-.02em]">Sign in to {name ?? (url ? new URL(url).host : "tt")}</h1>
          <div className="mt-[3px] font-mono text-[12px] text-muted" data-testid="login-server">
            {url ?? server}
          </div>
        </div>
        {conflict !== undefined && (
          <div
            role="alert"
            data-testid="login-conflict"
            className="flex w-full gap-[10px] rounded-md border border-[color-mix(in_srgb,var(--ctp-peach)_45%,transparent)] bg-[color-mix(in_srgb,var(--ctp-peach)_14%,var(--c-base))] p-3"
          >
            <span className="flex size-[18px] flex-none items-center justify-center rounded-full border-[1.5px] border-peach text-[11px] font-semibold text-peach-fg">!</span>
            <div className="flex flex-col gap-1 leading-[1.45]">
              <span className="font-medium">{ACCOUNT_CONFLICT}</span>
              <span className="text-[12px] text-sub1">
                &ldquo;{conflict}&rdquo; was created on another server first. This copy is blocked until renamed; its data is kept.
              </span>
              <span className="text-[12px] text-muted">
                Admin: <code className="font-mono text-[11.5px] text-fg">tt-server user rename {conflict} &lt;new-name&gt;</code>
              </span>
            </div>
          </div>
        )}
        <div className="flex w-full flex-col gap-3">
          <label className="block">
            <span className="mb-[5px] block text-[12px] text-sub1">Username</span>
            <span className={cn("tt-input gap-2 px-0", blocked && "border-peach")}>
              <input
                className="h-[34px] min-w-0 flex-1 bg-transparent px-[10px] outline-none"
                name="username"
                autoComplete="username"
                autoFocus
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
              {blocked && (
                <span aria-hidden className="px-[10px] text-[11px] text-muted">
                  blocked
                </span>
              )}
            </span>
          </label>
          <label className="block">
            <span className="mb-[5px] block text-[12px] text-sub1">Password</span>
            <span className="tt-input gap-2 px-0">
              <input
                className="h-[34px] min-w-0 flex-1 bg-transparent px-[10px] outline-none"
                name="password"
                type={show ? "text" : "password"}
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
              <button type="button" className="px-[10px] text-[11px] text-faint hover:text-fg" onClick={() => setShow(!show)}>
                {show ? "hide" : "show"}
              </button>
            </span>
          </label>
          <label className="flex items-center gap-2 text-[12px] text-sub1">
            <Checkbox checked={persist} onCheckedChange={(v) => setPersist(v === true)} aria-label="Stay signed in on this device" />
            Stay signed in on this device
          </label>
          {editServer && (
            <label className="block">
              <span className="mb-[5px] block text-[12px] text-sub1">Server</span>
              <input className="tt-input font-mono text-[12.5px]" value={server} onChange={(e) => setServer(e.target.value)} placeholder="https://tt.example.net" />
            </label>
          )}
        </div>
        {error && (
          <div role="alert" className="text-[12.5px] text-red-fg">
            {error}
          </div>
        )}
        <Button type="submit" variant="primary" size="lg" className="gap-[10px]" disabled={busy || blocked}>
          {busy ? "Signing in…" : "Sign in"} <Kbd inherit>⏎</Kbd>
        </Button>
        <div className="flex flex-col gap-[3px] text-[11.5px] text-faint">
          <span>
            Wrong server?{" "}
            <button type="button" className="text-link hover:underline" onClick={() => setEditServer(!editServer)}>
              change
            </button>{" "}
            · Sign in once; works offline after that.
          </span>
          <span>Your data is stored on this device and synced to the server when online.</span>
        </div>
      </form>
    </div>
  );
}
