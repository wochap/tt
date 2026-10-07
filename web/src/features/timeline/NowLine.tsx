import { formatClock } from "@tt/domain";

/** Red "now" line with a dot; the label sits in the hour gutter. Re-renders on a 30 s tick (caller). */
export function NowLine({ top, label, tz, now, showLabel = true }: { top: number; label?: string; tz: string; now: number; showLabel?: boolean }) {
  return (
    <div className="pointer-events-none absolute inset-x-0 z-20 border-t border-red" style={{ top }} data-testid="now-line">
      <span className="absolute -left-[5px] -top-[5px] size-[9px] rounded-full bg-red" />
      {showLabel && (
        <span className="absolute -left-[52px] -top-[7px] w-10 bg-base text-right font-mono text-[10.5px] leading-[14px] text-red-fg">
          {label ?? formatClock(tz, now)}
        </span>
      )}
    </div>
  );
}
