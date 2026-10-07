import { type Entry, formatDuration, type Range, totals } from "@tt/domain";

import { cn } from "@/lib/utils";

/** Summed, wall-clock (union), overlap and count over `entries` clipped to `range`. */
export function useTotals(entries: Entry[], now: number, range: Range) {
  return totals(entries, now, range);
}

export function TotalsGrid({ entries, now, range, running, title }: { entries: Entry[]; now: number; range: Range; running: number; title: string }) {
  const t = totals(entries, now, range);
  return (
    <div className="flex flex-col gap-[6px]" data-testid="totals">
      <div className="tt-label">{title}</div>
      <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-[3px] text-[12.5px] tabular-nums">
        <span className="text-muted">Tracked (summed)</span>
        <span className="font-medium" data-testid="total-summed">
          {formatDuration(t.summed)}
        </span>
        <span className="text-muted">Wall-clock</span>
        <span className="font-medium" data-testid="total-wall">
          {formatDuration(t.wall)}
        </span>
        <span className="text-muted">Overlap</span>
        <span className="text-peach-fg" data-testid="total-overlap">
          {formatDuration(t.overlap)}
        </span>
        <span className="text-muted">Entries</span>
        <span>
          {t.count} · {running} running
        </span>
      </div>
    </div>
  );
}

export function TotalsInline({ entries, now, range, className }: { entries: Entry[]; now: number; range: Range; className?: string }) {
  const t = totals(entries, now, range);
  return (
    <span className={cn("flex gap-[22px] tabular-nums", className)}>
      <span>
        <span className="text-muted">Summed </span>
        <span className="font-medium">{formatDuration(t.summed)}</span>
      </span>
      <span>
        <span className="text-muted">Wall-clock </span>
        <span className="font-medium">{formatDuration(t.wall)}</span>
      </span>
      <span>
        <span className="text-muted">Overlap </span>
        <span className="text-peach-fg">{formatDuration(t.overlap)}</span>
      </span>
    </span>
  );
}
