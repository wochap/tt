import { type ReactNode, useState } from "react";

import { addDays, type CivilDate, formatDayLong, formatDayShort, parseJumpDate, type Weekday } from "@tt/domain";

import { Calendar } from "@/components/ui/calendar";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

const EXAMPLES = ["-3d", "w42", "last fri", "next mon", "today"];

/** G: natural-language input above a calendar (frame 16). */
export function JumpToDate({
  open,
  onOpenChange,
  date,
  today,
  weekStart,
  onGo,
  children,
}: {
  children: ReactNode;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  date: CivilDate;
  today: CivilDate;
  weekStart: Weekday;
  /** `day`: also switch to day view (⇧⏎). */
  onGo: (date: CivilDate, day: boolean) => void;
}) {
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="flex items-baseline gap-2 rounded-2 px-1 text-[15px] font-medium tracking-[-.01em] hover:bg-[color-mix(in_srgb,var(--c-text)_5%,transparent)]"
          aria-label="Jump to date"
          data-testid="range-title"
        >
          {children}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-[300px] p-[10px]" onOpenAutoFocus={(e) => e.preventDefault()}>
        {open && <JumpBody date={date} today={today} weekStart={weekStart} onGo={(d, day) => (onGo(d, day), onOpenChange(false))} />}
      </PopoverContent>
    </Popover>
  );
}

function JumpBody({ date, today, weekStart, onGo }: { date: CivilDate; today: CivilDate; weekStart: Weekday; onGo: (date: CivilDate, day: boolean) => void }) {
  const [text, setText] = useState("");
  const [cursor, setCursor] = useState(date);
  const [month, setMonth] = useState(date);
  let parsed: CivilDate | undefined;
  try {
    parsed = text.trim() ? parseJumpDate(text, today, weekStart) : undefined;
  } catch {
    parsed = undefined;
  }
  const target = parsed ?? cursor;
  const move = (days: number) => {
    const next = addDays(cursor, days);
    setText("");
    setCursor(next);
    setMonth(next);
  };
  return (
    <div className="flex flex-col gap-[10px]">
      <label className="flex h-[34px] items-center gap-2 rounded-2 border border-accent bg-base px-[10px] text-[13px]">
        <input
          autoFocus
          aria-label="Date"
          value={text}
          placeholder={formatDayLong(date)}
          onChange={(e) => {
            setText(e.target.value);
            try {
              const d = parseJumpDate(e.target.value, today, weekStart);
              setMonth(d);
            } catch {
              // keep the calendar where it is while typing
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onGo(target, e.shiftKey);
            } else if (!text && e.key.startsWith("Arrow")) {
              e.preventDefault();
              move({ ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[e.key] ?? 0);
            }
          }}
          className="min-w-0 flex-1 bg-transparent caret-accent outline-none placeholder:text-faint"
        />
        <span className="ml-auto shrink-0 text-[11px] text-muted" data-testid="jump-preview">
          {text.trim() ? (parsed ? `→ ${formatDayShort(parsed)}` : "?") : ""}
        </span>
      </label>
      <div className="flex flex-wrap gap-[5px] text-[11px] text-muted">
        {EXAMPLES.map((example) => (
          <button key={example} type="button" onClick={() => setText(example)} className="rounded-2 border border-s1 px-[7px] py-[2px] font-mono hover:text-fg">
            {example}
          </button>
        ))}
      </div>
      <Calendar
        today={today}
        weekStart={weekStart}
        value={cursor}
        highlight={parsed ?? cursor}
        month={month}
        onMonthChange={setMonth}
        onSelect={(d, event) => onGo(d, event.shiftKey)}
      />
      <div className="flex gap-3 border-t pt-1 text-[11px] text-muted">
        <span>
          <Kbd>←→↑↓</Kbd> move
        </span>
        <span>
          <Kbd>⏎</Kbd> go
        </span>
        <span>
          <Kbd>⇧⏎</Kbd> go &amp; switch to day
        </span>
      </div>
    </div>
  );
}
