import { useState } from "react";
import { NavLink, useNavigate } from "react-router";

import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Kbd } from "@/components/ui/kbd";
import { useNow, useSession, useSyncStatus } from "@/data/react";
import { MOD, cn } from "@/lib/utils";
import { serverName } from "@/sync/protocol";

import { LogoutDialog } from "../settings/SettingsPage.tsx";
import { syncLabel } from "./SyncStatus.tsx";
import { useUi } from "./ui-state.tsx";

export const NAV = [
  { to: "/timeline", label: "Timeline", key: "1" },
  { to: "/tasks", label: "Tasks", key: "2" },
  { to: "/reports", label: "Reports", key: "3" },
  { to: "/settings", label: "Settings", key: "," },
] as const;

export function initials(name: string): string {
  const parts = name.split(/[\s._-]+/).filter(Boolean);
  if (parts.length >= 2) return (parts[0]![0]! + parts[1]![0]!).toLowerCase();
  return name.slice(0, 2).toLowerCase();
}

/** 44 px bar: brand, route tabs (1–3), command button (⌘K), account menu. */
export function TopBar() {
  const { openPalette } = useUi();
  return (
    <header className="flex h-11 flex-none items-center gap-[18px] border-b bg-mantle px-[14px]">
      <div className="text-[15px] font-semibold tracking-[-.02em] text-link">tt</div>
      <nav className="flex gap-[2px] text-[13px]" aria-label="Main">
        {NAV.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) =>
              cn("rounded-2 px-[9px] py-[5px] text-muted hover:text-fg", isActive && "bg-s0 text-fg")
            }
          >
            {item.label}
          </NavLink>
        ))}
      </nav>
      <div className="ml-auto flex items-center gap-[10px]">
        <button
          type="button"
          onClick={() => openPalette()}
          className="flex h-7 w-[240px] items-center justify-between rounded-2 border border-s0 bg-base px-[10px] text-[12px] text-faint hover:border-s1"
        >
          <span>Search or run a command…</span>
          <Kbd>{MOD}K</Kbd>
        </button>
        <AccountMenu />
      </div>
    </header>
  );
}

/** Frame 5.2: avatar menu naming the user and the server they sync with. */
export function AccountMenu() {
  const { auth } = useSession();
  const status = useSyncStatus();
  const now = useNow(5000);
  const navigate = useNavigate();
  const { setShortcutsOpen } = useUi();
  const [logout, setLogout] = useState(false);
  const name = serverName(auth);
  const { text, dot } = syncLabel(status, now);
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          aria-label={`${auth.user.name} on ${name}`}
          className="flex size-[26px] items-center justify-center rounded-full bg-s0 text-[11px] text-sub1 outline-none hover:text-fg focus-visible:shadow-[0_0_0_2px_var(--color-accent)]"
        >
          {initials(auth.user.name)}
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-[220px]">
          <div className="mb-1 flex flex-col gap-[3px] border-b border-s0 px-[9px] pb-[9px] pt-2" data-testid="account-identity">
            <span>
              <span className="font-medium">{auth.user.name}</span> <span className="text-muted">on</span>{" "}
              <span className="font-medium">{name}</span>
            </span>
            <span className="flex items-center gap-[6px] text-[11px] text-muted">
              <span className={cn("size-[6px] flex-none rounded-full", dot)} />
              {text}
            </span>
          </div>
          <DropdownMenuItem onSelect={() => void navigate("/settings")}>
            Settings
            <Kbd className="ml-auto">,</Kbd>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setShortcutsOpen(true)}>
            Keyboard shortcuts
            <Kbd className="ml-auto">?</Kbd>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setLogout(true)}>Sign out</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <LogoutDialog open={logout} onOpenChange={setLogout} pending={status.pending} />
    </>
  );
}

/** Phone: bottom tab bar (72 px) instead of the top tabs. */
export function PhoneNav() {
  return (
    <nav aria-label="Main" className="flex h-[72px] flex-none items-start justify-around border-t bg-mantle px-[10px] pt-[10px] text-[11px] text-muted">
      {NAV.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          className={({ isActive }) => cn("flex min-w-[60px] flex-col items-center gap-[3px]", isActive && "text-link")}
        >
          {({ isActive }) => (
            <>
              <span className={cn("h-[3px] w-[18px] rounded-xs", isActive ? "bg-current" : "bg-transparent")} />
              {item.label}
            </>
          )}
        </NavLink>
      ))}
    </nav>
  );
}
