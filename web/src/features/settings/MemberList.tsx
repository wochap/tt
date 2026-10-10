import { useNow } from "@/data/react";
import { cn } from "@/lib/utils";
import type { Endpoint, SyncStatus } from "@/sync/protocol";

import { ago } from "../shell/SyncStatus.tsx";

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/**
 * Every member server this device may sync with, the current one marked.
 * Members reporting a protocol this app does not speak are marked and skipped.
 */
export function MemberList({ status }: { status: SyncStatus }) {
  const now = useNow(5000);
  const members = status.members ?? [];
  if (members.length === 0) return <span className="text-[12.5px] text-muted">Known after the first connection.</span>;
  const current = status.member?.public_url;
  return (
    <ul className="flex flex-col gap-[6px] text-[12.5px]" data-testid="members-list">
      {members.map((member: Endpoint) => {
        const isCurrent = member.public_url === current;
        return (
          <li key={member.public_url} className="flex flex-wrap items-baseline gap-x-2" data-testid="member-row" data-current={isCurrent || undefined}>
            <span className={cn("font-medium", member.incompatible && "text-muted")}>{member.name}</span>
            <span className="font-mono text-[11px] text-faint">{host(member.public_url)}</span>
            {isCurrent && (
              <span className="rounded-sm bg-green/15 px-[6px] text-[11px] text-green-fg" data-testid="member-current">
                Syncing now
              </span>
            )}
            {member.incompatible ? (
              <span className="text-[11.5px] text-red-fg" data-testid="member-incompatible">
                Incompatible version
              </span>
            ) : (
              !isCurrent && <span className="text-[11.5px] text-muted">{member.last_ok ? `last synced ${ago(now - member.last_ok)}` : "never reached"}</span>
            )}
          </li>
        );
      })}
    </ul>
  );
}
