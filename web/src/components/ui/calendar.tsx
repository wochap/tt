import { CaretLeft, CaretRight } from "@phosphor-icons/react";
import { type MouseEvent, useState } from "react";

import {
  addDays,
  addMonths,
  type CivilDate,
  compareDates,
  DAY_NAMES,
  MONTH_NAMES_LONG,
  monthStart,
  sameDate,
  type Weekday,
  weekStartDate,
} from "@tt/domain";

import { cn } from "@/lib/utils";

/** Month grid (Popover + Calendar of the inventory), 5 or 6 rows, week start aware. */
export function Calendar({
  value,
  today,
  weekStart,
  onSelect,
  month: controlledMonth,
  onMonthChange,
  highlight,
}: {
  value?: CivilDate;
  today: CivilDate;
  weekStart: Weekday;
  onSelect: (date: CivilDate, event: MouseEvent) => void;
  month?: CivilDate;
  onMonthChange?: (month: CivilDate) => void;
  /** A proposed date (e.g. parsed from text) drawn with an accent ring. */
  highlight?: CivilDate;
}) {
  const [ownMonth, setOwnMonth] = useState(() => monthStart(value ?? today));
  const month = controlledMonth ? monthStart(controlledMonth) : ownMonth;
  const setMonth = (next: CivilDate) => (onMonthChange ? onMonthChange(next) : setOwnMonth(next));
  const first = weekStartDate(month, weekStart);
  const next = addMonths(month, 1);
  const rows = compareDates(addDays(first, 35), next) >= 0 ? 5 : 6;
  const days = Array.from({ length: rows * 7 }, (_, i) => addDays(first, i));
  const names = Array.from({ length: 7 }, (_, i) => DAY_NAMES[(weekStart + i) % 7]);
  return (
    <div className="flex flex-col gap-[10px]">
      <div className="flex items-center justify-between px-[2px] text-[12.5px] font-medium">
        <button type="button" aria-label="Previous month" className="tt-icon-btn size-6 border-transparent" onClick={() => setMonth(addMonths(month, -1))}>
          <CaretLeft size={12} />
        </button>
        <span>
          {MONTH_NAMES_LONG[month.month - 1]} {month.year}
        </span>
        <button type="button" aria-label="Next month" className="tt-icon-btn size-6 border-transparent" onClick={() => setMonth(addMonths(month, 1))}>
          <CaretRight size={12} />
        </button>
      </div>
      <div className="grid grid-cols-7 gap-[2px] text-center text-[10px] text-muted">
        {names.map((n) => (
          <span key={n}>{n}</span>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-[2px]" role="grid">
        {days.map((day) => {
          const inMonth = day.month === month.month;
          const isToday = sameDate(day, today);
          const isValue = value && sameDate(day, value);
          const isHighlight = highlight && sameDate(day, highlight);
          return (
            <button
              type="button"
              key={`${day.year}-${day.month}-${day.day}`}
              onClick={(event) => onSelect(day, event)}
              className={cn(
                "flex h-[30px] items-center justify-center rounded-2 text-[12px] tabular-nums hover:bg-[color-mix(in_srgb,var(--c-text)_7%,transparent)]",
                inMonth ? "text-fg" : "text-faint opacity-55",
                isToday && "bg-accent text-base hover:bg-accent",
                (isHighlight || (isValue && !isToday)) && "text-link shadow-[inset_0_0_0_1px_var(--color-accent)]",
              )}
            >
              {day.day}
            </button>
          );
        })}
      </div>
    </div>
  );
}
