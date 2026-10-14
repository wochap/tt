import { CaretUp } from "@phosphor-icons/react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Kbd } from "@/components/ui/kbd";
import { DURATION_GRIDS, type DurationGrid, gridLabel, type Grids, START_GRIDS, type StartGrid, startOptions, withDuration } from "@/lib/grids";

export function sameGrids(a: Grids, b: Grids): boolean {
  return a.start === b.start && a.duration === b.duration;
}

/** Grid chip (Turn 9 frame 17): Start and Duration radio groups, Reset to default, the ⌥ hint. */
export function GridMenu({ value, defaults, onChange }: { value: Grids; defaults: Grids; onChange: (value: Grids) => void }) {
  const overridden = !sameGrids(value, defaults);
  const valid = startOptions(value.duration);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="tt-chip-btn data-[state=open]:border-accent" data-testid="grid-menu">
        {overridden && <span data-testid="grid-overridden" aria-label="overridden" className="size-[6px] rounded-full bg-accent" />}
        <span>
          Start <span className="text-fg">{gridLabel(value.start)}</span> · Dur <span className="text-fg">{gridLabel(value.duration)}</span>
        </span>
        <CaretUp size={10} className="text-faint" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-[240px]">
        <DropdownMenuLabel className="tt-label">Start</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={String(value.start)} onValueChange={(v) => onChange({ ...value, start: Number(v) as StartGrid })}>
          {START_GRIDS.map((m) => {
            const disabled = !valid.includes(m);
            return (
              <DropdownMenuRadioItem key={m} value={String(m)} disabled={disabled}>
                {m === 0 ? "Off" : `${m} min`}
                {disabled && <span className="ml-auto text-[11px] text-faint">must divide duration</span>}
              </DropdownMenuRadioItem>
            );
          })}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuLabel className="tt-label">Duration</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={String(value.duration)} onValueChange={(v) => onChange(withDuration(value, Number(v) as DurationGrid))}>
          {DURATION_GRIDS.map((m) => (
            <DropdownMenuRadioItem key={m} value={String(m)}>
              {m} min
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem disabled={!overridden} onSelect={() => onChange(defaults)}>
          Reset to default
          <span className="ml-auto text-[11px] text-faint">
            {gridLabel(defaults.start)} · {gridLabel(defaults.duration)}
          </span>
        </DropdownMenuItem>
        <div className="mt-1 border-t px-[9px] pb-1 pt-[6px] text-[11px] leading-[1.5] text-muted">
          Hold <Kbd>⌥</Kbd> while dragging to disable snapping for that drag only. The defaults live in Settings.
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
