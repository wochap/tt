import { type Auth, type SyncStatus as Status, serverName } from "@/sync/protocol";

import { Tooltip } from "@/components/ui/tooltip";
import { useNow, useSession } from "@/data/react";
import { cn } from "@/lib/utils";

export function ago(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  return `${Math.round(m / 60)}h ago`;
}

const changes = (n: number) => `${n} change${n === 1 ? "" : "s"}`;

/** Turn 5 dot vocabulary: green synced, blue with halo syncing, hollow offline, red error. */
const DOT: Record<Status["state"], string> = {
  synced: "bg-green",
  syncing: "bg-blue shadow-[0_0_0_3px_color-mix(in_srgb,var(--ctp-blue)_28%,transparent)]",
  connecting: "bg-yellow",
  offline: "shadow-[inset_0_0_0_1.5px_var(--c-faint)]",
  "login-required": "bg-red",
  "signed-out": "shadow-[inset_0_0_0_1.5px_var(--c-faint)]",
};

/**
 * Status bar label naming the server, e.g. "Synced · laptop-a · 14s ago".
 * Without `server` (account menu, which names it already) the name is left out.
 */
export function syncLabel(status: Status, now: number, server?: string): { text: string; dot: string } {
  const dot = DOT[status.state];
  const at = server ? ` · ${server}` : "";
  switch (status.state) {
    case "synced":
      return { text: `Synced${at}${status.lastSync ? ` · ${ago(now - status.lastSync)}` : ""}`, dot };
    case "syncing":
      return { text: `Syncing ${changes(status.pending)}${at}`, dot };
    case "connecting":
      return { text: server ? `Connecting to ${server}…` : "Connecting…", dot };
    case "offline": {
      const head = server ? `${server} unreachable` : "Offline";
      return { text: status.pending ? `${head} · ${changes(status.pending)} saved here` : head, dot };
    }
    case "login-required":
      return { text: "Sign in again to sync", dot };
    case "signed-out":
      return { text: "Not signed in", dot };
  }
}

/** The member the app syncs with now, or the signed-in server while not connected. */
export function memberName(status: Status, auth: Auth): string {
  return status.member?.name ?? serverName(auth);
}

/** The name the status names: none while offline with several members (any of them would do). */
function labelServer(status: Status, auth: Auth): string | undefined {
  return status.state === "offline" && (status.members?.length ?? 0) > 1 ? undefined : memberName(status, auth);
}

/** Tooltip body (frame 5.1): server name and short id, last sync, pending, offline promise. */
export function SyncDetails({ status, now }: { status: Status; now: number }) {
  const { auth } = useSession();
  const name = memberName(status, auth);
  const id = status.member?.server_id || auth.identity?.id;
  const several = (status.members?.length ?? 0) > 1;
  const at = status.lastSync ? new Date(status.lastSync) : null;
  return (
    <div className="flex w-[260px] flex-col gap-[6px] p-1" data-testid="sync-details">
      <div className="flex items-center gap-2">
        <span className={cn("size-2 rounded-full", DOT[status.state])} />
        <span className="text-[13px] font-medium">{name}</span>
        {id && <span className="font-mono text-[11px] text-faint">{id.slice(0, 8)}</span>}
      </div>
      <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-[2px] text-muted">
        <span>Last sync</span>
        <span className="text-sub1">{at ? `${at.toLocaleTimeString([], { hourCycle: "h23" })} · ${ago(now - at.getTime())}` : "never"}</span>
        <span>Pending</span>
        <span className="text-sub1">{changes(status.pending)}</span>
      </div>
      <div className="text-muted">
        Changes are saved on this device and sync when {several ? "one of your servers" : name} is reachable.
      </div>
      {status.mode === "dedicated" && <div className="text-faint">This browser has no SharedWorker: each tab syncs on its own.</div>}
    </div>
  );
}

/**
 * Sync dot + label naming the server, never a blocking banner. Compact (phone
 * header) shows only the dot and the server name.
 */
export function SyncStatus({ status, compact = false }: { status: Status; compact?: boolean }) {
  const { auth } = useSession();
  const now = useNow(5000);
  const name = memberName(status, auth);
  const { text, dot } = syncLabel(status, now, labelServer(status, auth));
  return (
    <Tooltip content={<SyncDetails status={status} now={now} />}>
      <span
        data-testid="sync-status"
        data-state={status.state}
        aria-label={compact ? text : undefined}
        className={cn("flex items-center gap-[6px] whitespace-nowrap", compact && "h-6 rounded-2 border border-s1 px-[9px] text-sub1")}
      >
        <span className={cn("size-[6px] rounded-full", dot)} />
        {compact ? name : text}
      </span>
    </Tooltip>
  );
}
