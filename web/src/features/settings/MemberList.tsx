import { usePhone } from "@/lib/media";
import { cn } from "@/lib/utils";
import type { Endpoint, SyncStatus } from "@/sync/protocol";

import { type MemberState, memberState, memberStateLabel } from "../shell/member-state.ts";

/** Turn 6 member dots: green connected, green ring reachable, faint ring unreachable, peach skipped. */
const DOT: Record<MemberState, string> = {
  connected: "bg-green",
  reachable: "shadow-[inset_0_0_0_1.5px_var(--ctp-green)]",
  unreachable: "shadow-[inset_0_0_0_1.5px_var(--c-faint)]",
  skipped: "bg-peach",
};

function InUse() {
  return (
    <span className="rounded-sm px-[6px] py-px text-[10.5px] text-link shadow-[inset_0_0_0_1px_var(--color-accent)]" data-testid="member-current">
      in use
    </span>
  );
}

/**
 * Every member server this device may sync with: name, short id, URL and
 * state, the one in use marked. Read-only; the worker picks the member.
 */
export function MemberList({ status }: { status: SyncStatus }) {
  const phone = usePhone();
  const members = status.members ?? [];
  if (members.length === 0) return <span className="text-[12.5px] text-muted">Known after the first connection.</span>;
  return (
    <div className="flex flex-col gap-2">
      <ul className="flex flex-col text-[12.5px]" data-testid="members-list">
        {members.map((member: Endpoint) => {
          const state = memberState(member, status.member);
          const inUse = state === "connected";
          const id = member.server_id ? member.server_id.slice(0, 8) : "";
          const label = (
            <span className={cn("flex items-center gap-2", state === "unreachable" ? "text-muted" : "text-sub1")} data-testid="member-state">
              <span className={cn("size-2 flex-none rounded-full", DOT[state])} />
              {memberStateLabel(member, state)}
            </span>
          );
          return (
            <li
              key={member.public_url}
              data-testid="member-row"
              data-state={state}
              data-current={inUse || undefined}
              className={cn(
                "border-t border-s0 px-3",
                phone ? "flex min-h-[72px] flex-col justify-center gap-[2px] py-2" : "grid h-[38px] grid-cols-[220px_1fr_250px] items-center gap-3",
                inUse && "shadow-[inset_2px_0_0_var(--color-accent)]",
                inUse && !phone && "bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)]",
              )}
            >
              <span className="flex min-w-0 items-center gap-2">
                <span className="font-medium">{member.name}</span>
                {id && <span className="font-mono text-[11px] text-faint">{id}</span>}
                {inUse && <InUse />}
              </span>
              <span className={cn("min-w-0 truncate font-mono text-muted", phone ? "text-[11px]" : "text-[12px]")}>{member.public_url}</span>
              {label}
            </li>
          );
        })}
      </ul>
      <p className="max-w-[640px] text-[12px] leading-normal text-muted">
        {phone
          ? "Read-only. The app picks a member and switches by itself."
          : "Read-only. The app picks a member by itself and switches when the one in use stops answering. Members come from pairing on the servers."}
      </p>
    </div>
  );
}
