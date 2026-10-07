import type { SyncStatus as Status } from "@/sync/protocol";

import { Tooltip } from "@/components/ui/tooltip";
import { useNow } from "@/data/react";
import { cn } from "@/lib/utils";

function ago(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  return `${Math.round(m / 60)}h ago`;
}

export function syncLabel(status: Status, now: number): { text: string; dot: string } {
  const changes = (n: number) => `${n} change${n === 1 ? "" : "s"}`;
  switch (status.state) {
    case "synced":
      return { text: status.lastSync ? `Synced · ${ago(now - status.lastSync)}` : "Synced", dot: "bg-green" };
    case "syncing":
      return { text: `Syncing · ${changes(status.pending)}`, dot: "bg-yellow" };
    case "connecting":
      return { text: "Connecting…", dot: "bg-yellow" };
    case "offline":
      return { text: status.pending ? `Offline · ${changes(status.pending)} queued` : "Offline", dot: "bg-ov0" };
    case "login-required":
      return { text: "Sign in again to sync", dot: "bg-red" };
    case "signed-out":
      return { text: "Not signed in", dot: "bg-ov0" };
  }
}

/** Sync dot + label (synced | syncing | offline), never a blocking banner. */
export function SyncStatus({ status, compact = false }: { status: Status; compact?: boolean }) {
  const now = useNow(5000);
  const { text, dot } = syncLabel(status, now);
  const tip =
    status.mode === "dedicated"
      ? "This browser has no SharedWorker: each tab syncs on its own."
      : status.state === "offline"
        ? "Changes are saved on this device and sync when the server is reachable."
        : undefined;
  return (
    <Tooltip content={tip}>
      <span data-testid="sync-status" data-state={status.state} className="flex items-center gap-[6px] whitespace-nowrap">
        <span className={cn("size-[6px] rounded-full", dot)} />
        {compact ? text.split(" · ")[0]!.toLowerCase() : text}
      </span>
    </Tooltip>
  );
}
