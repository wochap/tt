import { type Entry, formatDuration, type Range, totals } from "@tt/domain";

import { cn } from "@/lib/utils";

/** Summed, wall-clock (union), overlap and count over `entries` clipped to `range`. */
export function useTotals(entries: Entry[], now: number, range: Range) {
  return totals(entries, now, range);
}

/** "3h 37m of 7h 29m" when `all` (the unfiltered figure) differs in kind from the shown one. */
function Of({ shown, all }: { shown: number; all?: number }) {
  return (
    <>
      {formatDuration(shown)}
      {all !== undefined && <span className="font-normal text-muted"> of {formatDuration(all)}</span>}
    </>
  );
}

/** `of` is the unfiltered entry list; when given, summed and wall-clock show "<filtered> of <all>". */
export function TotalsGrid({ entries, of, now, range, running, title }: { entries: Entry[]; of?: Entry[]; now: number; range: Range; running: number; title: string }) {
  const t = totals(entries, now, range);
  const all = of ? totals(of, now, range) : undefined;
  return (
    <div className="flex flex-col gap-[6px]" data-testid="totals">
      <div className="tt-label">{title}</div>
      {all && (
        <div className="text-[11px] text-link" data-testid="totals-filtered">
          Filtered · <Of shown={t.summed} all={all.summed} />
        </div>
      )}
      <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-[3px] text-[12.5px] tabular-nums">
        <span className="text-muted">Tracked (summed)</span>
        <span className="font-medium" data-testid="total-summed">
          <Of shown={t.summed} all={all?.summed} />
        </span>
        <span className="text-muted">Wall-clock</span>
        <span className="font-medium" data-testid="total-wall">
          <Of shown={t.wall} all={all?.wall} />
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

export function TotalsInline({ entries, of, now, range, className }: { entries: Entry[]; of?: Entry[]; now: number; range: Range; className?: string }) {
  const t = totals(entries, now, range);
  const all = of ? totals(of, now, range) : undefined;
  return (
    <span className={cn("flex gap-[22px] tabular-nums", className)}>
      {all && <span className="text-link">Filtered</span>}
      <span>
        <span className="text-muted">Summed </span>
        <span className="font-medium">
          <Of shown={t.summed} all={all?.summed} />
        </span>
      </span>
      <span>
        <span className="text-muted">Wall-clock </span>
        <span className="font-medium">
          <Of shown={t.wall} all={all?.wall} />
        </span>
      </span>
      <span>
        <span className="text-muted">Overlap </span>
        <span className="text-peach-fg">{formatDuration(t.overlap)}</span>
      </span>
    </span>
  );
}
