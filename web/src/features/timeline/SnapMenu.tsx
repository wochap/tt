import { CaretUp } from "@phosphor-icons/react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Kbd } from "@/components/ui/kbd";
import type { SnapMinutes } from "@/lib/settings";

const OPTIONS: { value: SnapMinutes; label: string; key: string }[] = [
  { value: 5, label: "5 min", key: "⌥1" },
  { value: 10, label: "10 min", key: "⌥2" },
  { value: 15, label: "15 min", key: "⌥3" },
];

/** Snap grid chip (frame 17): radio items, Off, and the ⌥ hint. */
export function SnapMenu({ value, onChange, hint = true }: { value: SnapMinutes; onChange: (value: SnapMinutes) => void; hint?: boolean }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="tt-chip-btn data-[state=open]:border-accent" data-testid="snap-menu">
        Snap <span className="text-fg">{value ? `${value}m` : "off"}</span>
        {hint && <span className="text-faint">· hold ⌥ to free</span>}
        <CaretUp size={10} className="text-faint" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-[230px]">
        <DropdownMenuRadioGroup value={String(value)} onValueChange={(v) => onChange(Number(v) as SnapMinutes)}>
          {OPTIONS.map((o) => (
            <DropdownMenuRadioItem key={o.value} value={String(o.value)}>
              {o.label}
              <Kbd className="ml-auto">{o.key}</Kbd>
            </DropdownMenuRadioItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuRadioItem value="0" className="text-muted">
            Off
            <Kbd className="ml-auto">⌥0</Kbd>
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <div className="mt-1 border-t px-[9px] pb-1 pt-[6px] text-[11px] leading-[1.5] text-muted">
          Hold <Kbd>⌥</Kbd> while dragging to disable snapping for that drag only. The default lives in Settings.
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
