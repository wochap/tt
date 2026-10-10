import { type ReactNode, useEffect, useState } from "react";

import { CopyButton } from "@/components/CopyButton";
import { fetchHealth } from "@/sync/server-api";

type Gate = { state: "checking" } | { state: "ready" } | { state: "needs-decision"; name: string };

/**
 * Checks `/api/health` before showing `children` (the login form). A server
 * that is not set up gets the not-set-up page instead, re-checked every
 * `recheckMs` until it reports ready. Unreachable counts as ready: the login
 * form reports that itself.
 */
export function SetupGate({ server, recheckMs = 10_000, children }: { server: string; recheckMs?: number; children: ReactNode }) {
  const [gate, setGate] = useState<Gate>({ state: "checking" });
  const waiting = gate.state === "needs-decision";

  useEffect(() => {
    if (gate.state === "ready") return;
    const controller = new AbortController();
    const check = async () => {
      const health = await fetchHealth(server, controller.signal);
      if (controller.signal.aborted) return;
      if (health?.setup === "needs-decision") setGate({ state: "needs-decision", name: health.server.name });
      // Keep the page while a re-check fails; the first check falls through to login.
      else if (health || !waiting) setGate({ state: "ready" });
    };
    // Do not hang on a silent network: after 5 s show the login form.
    const giveUp = waiting ? undefined : setTimeout(() => setGate((g) => (g.state === "checking" ? { state: "ready" } : g)), 5000);
    if (!waiting) void check();
    const timer = waiting ? setInterval(() => void check(), recheckMs) : undefined;
    return () => {
      controller.abort();
      clearTimeout(giveUp);
      clearInterval(timer);
    };
    // Re-run only on transitions between states, not on every name update.
  }, [server, recheckMs, waiting, gate.state]);

  if (gate.state === "checking") {
    return (
      <div className="flex h-full items-center justify-center text-[12px] text-muted" role="status">
        Starting…
      </div>
    );
  }
  if (gate.state === "needs-decision") return <NotSetUpPage name={gate.name} server={server} />;
  return children;
}

/** Frames 5.8 and 5.9: nobody can sign in until `init` or `peer join` has run. */
export function NotSetUpPage({ name, server }: { name: string; server: string }) {
  return (
    <main className="flex min-h-full flex-col justify-center gap-5 bg-base px-[22px] py-10 text-[13px] text-fg sm:gap-6 sm:px-24" data-testid="not-set-up">
      <div className="flex items-center gap-3">
        <img src="/icon.svg" alt="" className="size-9 sm:size-10" />
        <span className="inline-flex h-6 items-center gap-[6px] rounded-2 border border-s1 px-[9px] text-[12px] text-sub1">
          <span className="size-[6px] rounded-full shadow-[inset_0_0_0_1.5px_var(--c-faint)]" />
          {name}
        </span>
      </div>
      <div className="flex max-w-[560px] flex-col gap-2">
        <h1 className="text-[22px] font-medium tracking-[-.02em] sm:text-[28px]">{name} is not set up</h1>
        <p className="leading-[1.55] text-muted sm:text-[14px]">
          This server is running but has no data yet. Choose how to start from a terminal on {name}. Nobody can sign in until one of these has run.
        </p>
      </div>
      <div className="grid max-w-[720px] grid-cols-1 gap-[14px] sm:grid-cols-2">
        <Choice title="Start fresh" command="tt-server init">
          Creates an empty dataset and the first admin account.
        </Choice>
        <Choice title="Join another server" command="tt-server peer join <code>">
          Copies data and accounts from a paired server. Get a code there with <code className="font-mono text-[11.5px] text-fg">tt-server peer invite</code>.
        </Choice>
      </div>
      <div className="flex flex-wrap gap-x-[14px] gap-y-1 text-[12px] text-faint">
        <span className="font-mono">{server}</span>
        <span>This page reloads by itself when the server is ready.</span>
      </div>
    </main>
  );
}

function Choice({ title, command, children }: { title: string; command: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-[10px] rounded-md bg-mantle p-4">
      <span className="font-medium">{title}</span>
      <span className="text-[12.5px] leading-[1.5] text-muted">{children}</span>
      <div className="flex min-h-11 items-center justify-between gap-3 rounded-sm bg-base px-[10px] py-2 font-mono text-[12.5px] sm:min-h-0">
        <code data-testid="setup-command">{command}</code>
        <CopyButton text={command} />
      </div>
    </div>
  );
}
