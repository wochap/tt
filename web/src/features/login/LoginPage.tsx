import { type FormEvent, useState } from "react";

import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Checkbox } from "@/components/ui/switch";
import { defaultServer, login, normalizeServer } from "@/sync/auth";
import type { Auth } from "@/sync/protocol";

/** Frame 6: sign in once, works offline after that. */
export function LoginPage({ onLogin }: { onLogin: (auth: Auth, persist: boolean) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [show, setShow] = useState(false);
  const [persist, setPersist] = useState(true);
  const [server, setServer] = useState(defaultServer);
  const [editServer, setEditServer] = useState(false);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setError(undefined);
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
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-full items-center justify-center bg-base text-fg">
      <form onSubmit={submit} className="flex w-full max-w-[560px] flex-col items-start gap-[22px] px-6 py-10 sm:px-[72px]" aria-label="Sign in">
        <div>
          <div className="text-[26px] font-semibold tracking-[-.03em] text-link">tt</div>
          <div className="mt-[2px] text-[13px] text-muted">Sign in once. Works offline after that.</div>
        </div>
        <div className="flex w-full flex-col gap-3">
          <label className="block">
            <span className="mb-[5px] block text-[12px] text-sub1">Username</span>
            <input
              className="tt-input"
              name="username"
              autoComplete="username"
              autoFocus
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
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
        <Button type="submit" variant="primary" size="lg" className="gap-[10px]" disabled={busy}>
          {busy ? "Signing in…" : "Sign in"} <Kbd inherit>⏎</Kbd>
        </Button>
        <div className="flex flex-col gap-[3px] text-[11.5px] text-faint">
          <span>
            Server <span className="font-mono text-muted">{server}</span> ·{" "}
            <button type="button" className="text-link hover:underline" onClick={() => setEditServer(!editServer)}>
              change
            </button>
          </span>
          <span>Your data is stored on this device and synced to the server when online.</span>
        </div>
      </form>
    </div>
  );
}
