import { useMemo, useState } from "react";

import {
  addDays,
  type CivilDate,
  compareDates,
  DAY_NAMES,
  type Entry,
  formatDuration,
  formatDate,
  monthStart,
  addMonths,
  sameDate,
  totals,
  type Weekday,
  weekStartDate,
  type View,
  weekday,
} from "@tt/domain";

import { taskColor } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { cn } from "@/lib/utils";

import { columnFor } from "./geometry.ts";

interface Cell {
  date: CivilDate;
  key: string;
  inMonth: boolean;
  summed: number;
  wall: number;
  top: { task: string; seconds: number }[];
}

/** Per-day totals over the month grid, computed once per change of the entries. */
export function monthCells(tz: string, month: CivilDate, weekStart: Weekday, now: number, entries: Entry[]): Cell[] {
  const first = weekStartDate(monthStart(month), weekStart);
  const next = addMonths(monthStart(month), 1);
  const rows = compareDates(addDays(first, 35), next) >= 0 ? 5 : 6;
  return Array.from({ length: rows * 7 }, (_, i) => {
    const date = addDays(first, i);
    const column = columnFor(tz, date);
    const range = { from: column.from, to: column.to };
    const day = entries.filter((e) => e.start < column.to && (e.end ?? now) > column.from);
    const t = totals(day, now, range);
    const byTask = new Map<string, number>();
    for (const e of day) {
      const s = totals([e], now, range).summed;
      byTask.set(e.task, (byTask.get(e.task) ?? 0) + s);
    }
    const top = [...byTask.entries()].sort((a, b) => b[1] - a[1]).map(([task, seconds]) => ({ task, seconds }));
    return { date, key: formatDate(date), inMonth: date.month === month.month, summed: t.summed, wall: t.wall, top };
  });
}

/** Frame 3: day totals, a stacked bar of the top tasks, the top N tasks; click or ⏎ opens the day. */
export function MonthView({
  view,
  tz,
  month,
  today,
  weekStart,
  now,
  entries,
  topTasks,
  onOpenDay,
}: {
  view: View;
  tz: string;
  month: CivilDate;
  today: CivilDate;
  weekStart: Weekday;
  now: number;
  entries: Entry[];
  topTasks: number;
  onOpenDay: (date: CivilDate) => void;
}) {
  const cells = useMemo(
    () => monthCells(tz, month, weekStart, now, entries),
    // Totals move by the minute at most.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [view, tz, month.year, month.month, weekStart, Math.floor(now / 60_000), entries],
  );
  const [focus, setFocus] = useState<CivilDate>(() => (today.month === month.month && today.year === month.year ? today : monthStart(month)));
  const names = Array.from({ length: 7 }, (_, i) => DAY_NAMES[(weekStart + i) % 7]);
  const rows = cells.length / 7;
  const max = 9 * 3600;

  const moveFocus = (days: number) => {
    const next = addDays(focus, days);
    if (cells.some((c) => sameDate(c.date, next))) setFocus(next);
  };
  useKeys({
    "h": () => moveFocus(-1),
    "l": () => moveFocus(1),
    "k|arrowup": () => moveFocus(-7),
    "j|arrowdown": () => moveFocus(7),
    "enter": () => onOpenDay(focus),
  });

  return (
    <div className="flex min-h-0 flex-1 flex-col pl-[14px] pr-[18px]">
      <div className="grid h-7 flex-none grid-cols-7 items-end pb-1">
        {names.map((n) => (
          <span key={n} className="px-2 text-[11px] uppercase tracking-[.06em] text-muted">
            {n}
          </span>
        ))}
      </div>
      <div
        className="grid min-h-0 flex-1 grid-cols-7 border-l border-t"
        style={{ gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }}
        role="grid"
        aria-label="Month"
      >
        {cells.map((cell) => {
          const isToday = sameDate(cell.date, today);
          const future = compareDates(cell.date, today) > 0;
          const weekend = weekday(cell.date) >= 5;
          const focused = sameDate(cell.date, focus);
          const top = cell.top.slice(0, topTasks);
          let acc = 0;
          const stops = top
            .map(({ task, seconds }) => {
              const color = taskColor(view.workspace, view.workspace.tasks.get(task));
              const a = (acc / Math.max(cell.summed, 1)) * 100;
              acc += seconds;
              const b = (acc / Math.max(cell.summed, 1)) * 100;
              return `${color} ${a}% ${b}%`;
            })
            .join(", ");
          return (
            <button
              type="button"
              role="gridcell"
              key={cell.key}
              data-date={cell.key}
              data-testid="month-cell"
              onClick={() => onOpenDay(cell.date)}
              onFocus={() => setFocus(cell.date)}
              tabIndex={focused ? 0 : -1}
              className={cn(
                "flex min-w-0 flex-col overflow-hidden border-b border-r px-2 py-[6px] text-left hover:bg-[color-mix(in_srgb,var(--c-text)_4%,transparent)]",
                !cell.inMonth && "opacity-55",
                isToday && "bg-[color-mix(in_srgb,var(--color-accent)_5%,transparent)]",
                !isToday && weekend && "tt-hatch",
                focused && "outline-2 -outline-offset-2 outline-accent",
              )}
            >
              <div className="flex items-baseline justify-between">
                <span
                  className={cn(
                    "text-[12px]",
                    isToday ? "rounded-sm bg-accent px-[5px] font-medium leading-[18px] text-base" : future ? "text-faint" : "text-sub1",
                  )}
                >
                  {cell.date.day}
                </span>
                {cell.summed > 0 && <span className="text-[12px] font-medium tabular-nums text-sub1">{formatDuration(cell.summed)}</span>}
              </div>
              <div
                className="mt-[6px] h-[3px] rounded-xs"
                style={cell.summed > 0 ? { width: `${Math.min(100, Math.round((cell.summed / max) * 100))}%`, background: stops ? `linear-gradient(to right, ${stops})` : "var(--c-ov1)" } : undefined}
              />
              <div className="mt-1 flex min-w-0 flex-col gap-[2px]">
                {top.map(({ task, seconds }) => {
                  const record = view.workspace.tasks.get(task);
                  return (
                    <div key={task} className="flex min-w-0 items-center gap-[6px] text-[11.5px]">
                      <span className="size-[7px] shrink-0 rounded-xs" style={{ background: taskColor(view.workspace, record) }} />
                      <span className="min-w-0 flex-1 truncate">{record?.title ?? "(deleted task)"}</span>
                      <span className="shrink-0 tabular-nums text-faint">{formatDuration(seconds)}</span>
                    </div>
                  );
                })}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function monthSummary(cells: Cell[]) {
  const tracked = cells.filter((c) => c.inMonth && c.summed > 0);
  const summed = tracked.reduce((a, c) => a + c.summed, 0);
  const wall = tracked.reduce((a, c) => a + c.wall, 0);
  return { days: tracked.length, summed, wall, avg: tracked.length ? Math.round(summed / tracked.length) : 0 };
}
