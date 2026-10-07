import { Outlet, useNavigate } from "react-router";

import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useActions } from "@/data/actions";
import { useSnapshot, useSyncStatus } from "@/data/react";
import { useKeys } from "@/lib/keys";
import { usePhone } from "@/lib/media";

import { CommandPalette } from "./CommandPalette.tsx";
import { RunningStrip } from "./RunningStrip.tsx";
import { ShortcutSheet } from "./ShortcutSheet.tsx";
import { StatusBar } from "./StatusBar.tsx";
import { SyncStatus } from "./SyncStatus.tsx";
import { PhoneNav, TopBar } from "./TopBar.tsx";
import { UiProvider, useUi } from "./ui-state.tsx";

export function AppShell() {
  return (
    <UiProvider>
      <TooltipProvider delayDuration={400}>
        <Shell />
        <CommandPalette />
        <ShortcutSheet />
        <Toaster />
      </TooltipProvider>
    </UiProvider>
  );
}

function GlobalKeys() {
  const navigate = useNavigate();
  const actions = useActions();
  const ui = useUi();
  useKeys(
    {
      "mod+k": () => (ui.palette.open ? ui.closePalette() : ui.openPalette()),
      "?": () => ui.setShortcutsOpen(!ui.shortcutsOpen),
      "shift+mod+z|mod+y": () => void actions.redo(),
      "mod+z": () => void actions.undo(),
      "1": () => void navigate("/timeline"),
      "2": () => void navigate("/tasks"),
      "3": () => void navigate("/reports"),
      ",": () => void navigate("/settings"),
      "shift+s": () => actions.stopAll(),
      c: () => ui.openPalette("create"),
    },
    { enabled: !ui.palette.open },
  );
  return null;
}

function Shell() {
  const phone = usePhone();
  const snapshot = useSnapshot();
  const status = useSyncStatus();
  const body =
    snapshot.state === "ready" ? (
      <Outlet />
    ) : snapshot.state === "error" ? (
      <div className="m-auto max-w-[420px] p-6 text-center text-[13px] text-muted">
        <div className="mb-2 text-[15px] font-medium text-fg">Cannot open your data</div>
        {snapshot.error}
      </div>
    ) : (
      <div className="m-auto text-[12px] text-muted" role="status">
        Loading your data…
      </div>
    );
  if (phone) {
    return (
      <div className="flex h-full flex-col bg-base">
        <GlobalKeys />
        <div className="flex h-[38px] flex-none items-end justify-between px-5 pb-[6px] text-[12px] text-muted">
          <span className="font-semibold text-link">tt</span>
          <SyncStatus status={status} compact />
        </div>
        {snapshot.state === "ready" && <RunningStrip phone />}
        <main className="flex min-h-0 flex-1 flex-col">{body}</main>
        <PhoneNav />
      </div>
    );
  }
  return (
    <div className="flex h-full flex-col bg-base">
      <GlobalKeys />
      <TopBar />
      {snapshot.state === "ready" && <RunningStrip />}
      <main className="flex min-h-0 flex-1 flex-col">{body}</main>
      <StatusBar />
    </div>
  );
}
