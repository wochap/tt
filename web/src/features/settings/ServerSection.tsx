import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";

import { CopyButton } from "@/components/CopyButton";
import { useNow, useSession } from "@/data/react";
import { usePhone } from "@/lib/media";
import { cn } from "@/lib/utils";
import { serverName } from "@/sync/protocol";
import { fetchHealth, fetchPeers, type Health, type Peer, type Peers, type PeerState } from "@/sync/server-api";

import { ago } from "../shell/SyncStatus.tsx";

interface ServerStatus {
  health: Health | null;
  peers: Peers | null;
  /** The last `/api/peers` request failed: what is shown is the previous result. */
  stale: boolean;
  /** When `peers` was loaded (ms since epoch). */
  at: number | null;
}

const EMPTY: ServerStatus = { health: null, peers: null, stale: false, at: null };

// Last result per session, kept in memory so reopening Settings offline still
// shows the peers (marked stale) instead of nothing.
const cache = new Map<string, ServerStatus>();

/** Drops the in-memory peers cache (tests). */
export function forgetServerStatus(): void {
  cache.clear();
}

/** Turn 5 dot vocabulary: green online, blue with halo syncing, hollow offline, red error. */
const DOT: Record<PeerState, string> = {
  online: "bg-green",
  syncing: "bg-blue shadow-[0_0_0_3px_color-mix(in_srgb,var(--ctp-blue)_28%,transparent)]",
  offline: "shadow-[inset_0_0_0_1.5px_var(--c-faint)]",
  error: "bg-red",
};

function stateLabel(peer: Peer): string {
  switch (peer.state) {
    case "online":
      return "Online · in sync";
    case "syncing":
      return `Syncing · ${peer.pending} doc${peer.pending === 1 ? "" : "s"} pending`;
    case "offline":
      return "Offline";
    case "error":
      return `Error · ${peer.error ?? "unknown"}`;
  }
}

const stateColor = (state: PeerState) => (state === "error" ? "text-red-fg" : state === "offline" ? "text-muted" : "text-sub1");

function seen(peer: Peer, now: number): string {
  if (peer.last_seen === null) return "never";
  const ms = now - peer.last_seen * 1000;
  return ms < 10_000 ? "now" : ago(ms);
}

/** True while `ref` is on screen and the tab is visible; without IntersectionObserver, while the tab is visible. */
function useVisible(ref: RefObject<HTMLElement | null>): boolean {
  const [onScreen, setOnScreen] = useState(() => typeof IntersectionObserver === "undefined");
  const [tabVisible, setTabVisible] = useState(() => document.visibilityState !== "hidden");
  useEffect(() => {
    const element = ref.current;
    if (!element || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => setOnScreen(entries.some((entry) => entry.isIntersecting)));
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  useEffect(() => {
    const update = () => setTabVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  return onScreen && tabVisible;
}

/**
 * Frames 5.4 to 5.7: this server's identity and a read-only list of its peers
 * (pairing happens in the CLI). Refreshes every `refreshMs` only while the
 * section is on screen; a failed refresh keeps the last list, marked stale.
 */
export function ServerSection({ refreshMs = 10_000, server: current }: { refreshMs?: number; server?: string }) {
  const { auth } = useSession();
  // The member the app syncs with now; the signed-in server until connected.
  const server = current ?? auth.server;
  const phone = usePhone();
  const now = useNow(5000);
  const ref = useRef<HTMLElement>(null);
  const visible = useVisible(ref);
  const key = `${server} ${auth.token}`;
  const [status, setStatus] = useState<ServerStatus>(() => cache.get(key) ?? EMPTY);

  useEffect(() => {
    if (!visible) return;
    const controller = new AbortController();
    const load = async () => {
      const [health, peers] = await Promise.all([
        fetchHealth(server, controller.signal),
        fetchPeers({ server, token: auth.token }, controller.signal),
      ]);
      if (controller.signal.aborted) return;
      setStatus((previous) => {
        const next: ServerStatus = {
          health: health ?? previous.health,
          peers: peers ?? previous.peers,
          stale: !peers,
          at: peers ? Date.now() : previous.at,
        };
        cache.set(key, next);
        return next;
      });
    };
    void load();
    const timer = setInterval(() => void load(), refreshMs);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [visible, server, auth.token, key, refreshMs]);

  const signedIn = server === auth.server;
  const name = status.peers?.server.name ?? status.health?.server.name ?? (signedIn ? serverName(auth) : new URL(server).host);
  const id = status.peers?.server.id || status.health?.server.id || (signedIn ? auth.identity?.id : "") || "";
  const short = id.slice(0, 8);
  const peers = status.peers?.peers;
  const reach = peers?.filter((p) => p.state === "online" || p.state === "syncing").length ?? 0;

  return (
    <section ref={ref} id="settings-server" aria-label="Server" className="flex scroll-mt-4 flex-col gap-[14px]">
      <h2 className="tt-label">Server</h2>
      <div className="grid grid-cols-1 items-center gap-x-5 gap-y-[14px] text-[13px] sm:grid-cols-[180px_1fr]">
        <span className="text-sub1">Name</span>
        <span className="font-medium" data-testid="server-name">
          {name}
        </span>
        <span className="text-sub1">Server id</span>
        <span className="flex items-center gap-2">
          {short ? (
            <>
              <span className="rounded-sm border bg-mantle px-2 py-[3px] font-mono text-[12.5px]" data-testid="server-id">
                {short}
              </span>
              <CopyButton text={short} className="text-[12px]" />
            </>
          ) : (
            <span className="text-faint">unknown</span>
          )}
        </span>
        <span className="text-sub1">Version</span>
        <span className="font-mono text-[12.5px]" data-testid="server-version">
          {status.health?.version ? `tt-server ${status.health.version}` : <span className="font-sans text-faint">unknown</span>}
        </span>
      </div>

      <div className="flex flex-col gap-[10px]">
        <div className="flex flex-wrap items-baseline gap-[10px]">
          <span className="tt-label">Peers</span>
          {peers && peers.length > 0 && (
            <span className="text-[12px] text-faint">
              {peers.length} peer{peers.length === 1 ? "" : "s"} · {reach} reachable
            </span>
          )}
          {status.stale && (
            <span className="ml-auto flex items-center gap-[6px] text-[11.5px] text-muted" data-testid="peers-stale">
              <span className="size-[6px] rounded-full shadow-[inset_0_0_0_1.5px_var(--c-faint)]" />
              {status.at ? `Stale · ${name} unreachable · updated ${ago(now - status.at)}` : `${name} unreachable`}
            </span>
          )}
        </div>
        {!peers ? (
          <div className="text-[12.5px] text-muted">{status.stale ? "Peers are shown once the server is reachable." : "Loading peers…"}</div>
        ) : peers.length === 0 ? (
          <EmptyPeers name={name} phone={phone} />
        ) : (
          <>
            <div className={cn(status.stale && "opacity-70")}>{phone ? <PeerList peers={peers} now={now} /> : <PeerTable peers={peers} now={now} />}</div>
            <div className="flex flex-wrap items-center gap-2 text-[12px] leading-[1.6] text-muted">
              Pair from a terminal on this machine:<Cmd>tt-server peer invite</Cmd>then on the other server<Cmd>tt-server peer join &lt;code&gt;</Cmd>
            </div>
          </>
        )}
      </div>
    </section>
  );
}

function Cmd({ children, className }: { children: ReactNode; className?: string }) {
  return <code className={cn("rounded-sm bg-mantle px-[7px] py-[2px] font-mono text-[11.5px] text-fg", className)}>{children}</code>;
}

function Dot({ state }: { state: PeerState }) {
  return <span aria-hidden className={cn("size-2 flex-none rounded-full", DOT[state])} />;
}

/** Desktop (frame 5.4): Server, State with the link address, Last seen. */
function PeerTable({ peers, now }: { peers: Peer[]; now: number }) {
  return (
    <table className="w-full table-fixed overflow-hidden rounded-md border text-[13px]" data-testid="peers-table">
      <thead className="bg-mantle text-left text-[11px] uppercase tracking-[.06em] text-muted">
        <tr className="h-8">
          <th className="w-[38%] px-3 font-normal">Server</th>
          <th className="px-3 font-normal">State</th>
          <th className="w-[100px] px-3 text-right font-normal">Last seen</th>
        </tr>
      </thead>
      <tbody>
        {peers.map((peer) => (
          <tr key={peer.id} className="h-[38px] border-t" data-testid="peer-row">
            <td className="px-3">
              <span className="flex min-w-0 items-baseline gap-2">
                <span className="truncate font-medium">{peer.name}</span>
                <span className="font-mono text-[11px] text-faint">{peer.id.slice(0, 8)}</span>
              </span>
            </td>
            <td className="px-3">
              <span className={cn("flex min-w-0 items-center gap-2", stateColor(peer.state))}>
                <Dot state={peer.state} />
                <span className="truncate">{stateLabel(peer)}</span>
                {peer.address && (
                  <span className="truncate font-mono text-[11px] text-faint" title={peer.address}>
                    · {peer.address}
                  </span>
                )}
              </span>
            </td>
            <td className="px-3 text-right text-muted tabular-nums">{seen(peer, now)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Phone (frame 5.5): one stacked row per peer. */
function PeerList({ peers, now }: { peers: Peer[]; now: number }) {
  return (
    <ul className="flex flex-col rounded-md bg-mantle" data-testid="peers-list">
      {peers.map((peer) => (
        <li key={peer.id} className="flex min-h-14 items-center gap-3 border-b px-3 py-2 last:border-b-0" data-testid="peer-row">
          <Dot state={peer.state} />
          <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
            <span className="font-medium">{peer.name}</span>
            <span className={cn("text-[12px]", stateColor(peer.state))}>{stateLabel(peer)}</span>
            {peer.address && <span className="truncate font-mono text-[11px] text-faint">{peer.address}</span>}
          </div>
          <span className="text-[12px] text-muted tabular-nums">{seen(peer, now)}</span>
        </li>
      ))}
    </ul>
  );
}

/** Frames 5.6 and 5.7. */
function EmptyPeers({ name, phone }: { name: string; phone: boolean }) {
  return (
    <div
      className={cn("flex flex-col gap-2", phone ? "gap-[6px] rounded-md bg-mantle p-[14px]" : "rounded-md border border-dashed border-s1 p-[18px]")}
      data-testid="peers-empty"
    >
      <div className={cn("font-medium", phone ? "text-[14px]" : "text-[15px]")}>Not paired with other servers</div>
      <div className={cn("max-w-[480px] leading-[1.5] text-muted", phone && "text-[12.5px]")}>
        {phone
          ? `Pairing lets ${name} sync with another tt server when they can reach each other.`
          : `Pairing lets ${name} copy tasks and entries to and from another tt server whenever they can reach each other over LAN or Tailscale.`}
      </div>
      <div className="mt-1 flex flex-wrap items-center gap-2 text-[12px] text-muted">
        <Cmd className={phone ? "bg-base" : undefined}>tt-server peer invite</Cmd>
        here, then
        <Cmd className={phone ? "bg-base" : undefined}>tt-server peer join &lt;code&gt;</Cmd>
        on the other server
      </div>
    </div>
  );
}
