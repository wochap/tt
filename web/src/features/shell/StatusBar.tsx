import { useSyncStatus } from "@/data/react";
import { useTz } from "@/lib/settings";

import { SyncStatus } from "./SyncStatus.tsx";
import { useUi } from "./ui-state.tsx";

/** 24 px footer: sync state with the server name, zone, then the current view's key hints. */
export function StatusBar() {
  const status = useSyncStatus();
  const tz = useTz();
  const { hints } = useUi();
  return (
    <footer className="flex h-6 flex-none items-center gap-[14px] overflow-hidden whitespace-nowrap border-t bg-mantle px-[14px] text-[11px] text-faint">
      <SyncStatus status={status} />
      <span>{tz}</span>
      <span className="ml-auto flex gap-[10px]">{hints}</span>
    </footer>
  );
}
