import { NavLink } from "react-router";

import { Kbd } from "@/components/ui/kbd";
import { useSession } from "@/data/react";
import { MOD, cn } from "@/lib/utils";

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

/** 44 px bar: brand, route tabs (1–3), command button (⌘K), avatar. */
export function TopBar() {
  const { auth } = useSession();
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
        <div
          title={auth.user.name}
          className="flex size-[26px] items-center justify-center rounded-full bg-s0 text-[11px] text-sub1"
        >
          {initials(auth.user.name)}
        </div>
      </div>
    </header>
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
