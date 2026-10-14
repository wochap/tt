import { CaretLeft, CaretRight } from "@phosphor-icons/react";
import { useEffect, useMemo, useRef, useState } from "react";

import { formatClock, formatDayShort, formatDuration, parseDate, type Range, type RoundGroup, type RoundInput, type Rounded, type View, type Weekday } from "@tt/domain";

import { taskColor } from "@/lib/colors";
import { usePhone } from "@/lib/media";
import { cn } from "@/lib/utils";

import { type Badge, type DaySection, type FlatRow, type PairRow, previewModel, type RawRow, type WeekSection } from "./preview.ts";

const BADGE_STYLE: Record<Badge, string> = {
  moved: "color-mix(in srgb, var(--ctp-peach) 18%, var(--c-base))",
  snapped: "color-mix(in srgb, var(--ctp-sky) 16%, var(--c-base))",
  rounded: "var(--c-s0)",
  dropped: "var(--c-s0)",
};

/** Pinned day header: the day section crossing the top edge of the scroller. */
function usePinnedDay(scroller: React.RefObject<HTMLDivElement | null>, keys: string, onPinned: (day: string) => void) {
  const callback = useRef(onPinned);
  callback.current = onPinned;
  useEffect(() => {
    const root = scroller.current;
    if (!root || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const day = (entry.target as HTMLElement).dataset.day;
          if (entry.isIntersecting && day) callback.current(day);
        }
      },
      // A one-pixel line at the top of the scroller.
      { root, rootMargin: "0px 0px -99% 0px", threshold: 0 },
    );
    for (const el of root.querySelectorAll<HTMLElement>("[data-day]")) observer.observe(el);
    return () => observer.disconnect();
  }, [scroller, keys]);
}

/**
 * Turn 9 frame 10 and 9.1–9.6: raw and rounded rows paired in one scrolling
 * table, with sticky day headers and week subtotals over multi-day ranges.
 * Every number comes from the shared `roundAndFlatten` output it is given, so
 * the preview equals the downloaded export.
 */
export function RoundFlattenPreview({
  view,
  tz,
  inputs,
  rounded,
  group,
  gridMinutes,
  mode,
  startGridMinutes,
  weekStart,
  range,
}: {
  view: View;
  tz: string;
  inputs: readonly RoundInput[];
  rounded: readonly Rounded[];
  group: RoundGroup;
  gridMinutes: number;
  mode: "up" | "nearest";
  startGridMinutes: number;
  weekStart: Weekday;
  range: Range;
}) {
  const phone = usePhone();
  const model = useMemo(() => previewModel(inputs, rounded, group, tz, weekStart, range), [inputs, rounded, group, tz, weekStart, range]);
  const daySections = model.sections.filter((s): s is DaySection => s.kind === "day");
  const days = daySections.map((s) => s.day);
  const [picked, setPicked] = useState<string>();
  const day = picked && days.includes(picked) ? picked : days[0];
  const [hovered, setHovered] = useState<string>();
  const scroller = useRef<HTMLDivElement>(null);
  usePinnedDay(scroller, days.join(","), setPicked);
  const title = (task: string) => view.workspace.tasks.get(task)?.title ?? "(deleted task)";
  const color = (task: string) => taskColor(view.workspace, view.workspace.tasks.get(task));
  const section = daySections.find((s) => s.day === day);

  const step = (to: string) => {
    setPicked(to);
    scroller.current?.querySelector(`[data-day="${to}"]`)?.scrollIntoView?.({ block: "start" });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="round-preview">
      <div className="flex items-center gap-2 px-[18px] pt-[10px] text-[11px] text-faint tabular-nums">
        <span data-testid="rows-note" className="ml-auto">
          {model.downloadRows} rows in download{model.droppedRows ? ` · ${model.droppedRows} dropped` : ""}
        </span>
      </div>
      {section && (
        <Strips
          tz={tz}
          section={section}
          days={days}
          onDay={step}
          color={color}
          gridMinutes={gridMinutes}
          mode={mode}
          startGridMinutes={startGridMinutes}
        />
      )}
      <div className="relative mx-[18px] mt-3 flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-s0 bg-[linear-gradient(to_right,var(--c-mantle)_50%,var(--c-base)_50%)] max-md:bg-base">
        <div className="tt-label grid flex-none grid-cols-2 border-b max-md:grid-cols-1">
          <div className="flex justify-between px-[10px] py-[7px] max-md:hidden">
            <span>Raw · {inputs.length} entries</span>
            <span className="normal-case tracking-normal tabular-nums text-fg" data-testid="raw-total">
              {formatDuration(model.rawTotal)}
            </span>
          </div>
          <div className="flex justify-between border-l border-accent px-[10px] py-[7px] text-link max-md:border-l-0">
            <span>{group === "entry" ? "Rounded & flattened" : "Rounded · per task per day"}</span>
            <span className="normal-case tracking-normal tabular-nums text-fg" data-testid="flat-total">
              {formatDuration(model.flatTotal)}
            </span>
          </div>
        </div>
        <div ref={scroller} className="tt-scroll relative min-h-0 flex-1 overflow-y-auto" data-testid="preview-scroll">
          {model.sections.map((s) =>
            s.kind === "week" ? (
              <WeekRow key={s.key} section={s} phone={phone} />
            ) : (
              <div key={s.day} data-day={s.day} data-testid="day-section">
                {model.multiDay && <DayHeader section={s} active={s.day === day} phone={phone} onPick={() => setPicked(s.day)} />}
                {s.rows.map((row) =>
                  phone ? (
                    <PhoneRow key={row.key} row={row} tz={tz} title={title} color={color} />
                  ) : (
                    <Pair key={row.key} row={row} tz={tz} title={title} color={color} group={group} hovered={hovered === row.key} onHover={(on) => setHovered(on ? row.key : undefined)} />
                  ),
                )}
              </div>
            ),
          )}
        </div>
      </div>
      {group === "task-day" && <div className="px-[18px] pt-2 text-[11px] text-muted">Same minutes, one row per task and day; each group starts at its first entry.</div>}
    </div>
  );
}

function DayHeader({ section, active, phone, onPick }: { section: DaySection; active: boolean; phone: boolean; onPick: () => void }) {
  const count = section.rows.filter((r) => r.raw).length;
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onPick}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onPick()}
      data-testid="day-header"
      className="sticky top-0 z-[2] grid h-[30px] cursor-pointer grid-cols-2 border-b border-s1 text-[12px] tabular-nums max-md:grid-cols-1"
    >
      <div className="flex items-center gap-2 bg-[color-mix(in_srgb,var(--c-s0)_70%,var(--c-mantle))] px-[10px]">
        <span className={cn("font-medium", active && "text-link")}>{section.label}</span>
        <span className="text-[11px] text-faint">
          {count} {count === 1 ? "entry" : "entries"}
        </span>
        {!phone && <span className="ml-auto font-medium">{formatDuration(section.rawSub)}</span>}
        {phone && (
          <span className="ml-auto font-medium">
            <span className="text-faint">{formatDuration(section.rawSub)} → </span>
            {formatDuration(section.flatSub)}
          </span>
        )}
      </div>
      {!phone && (
        <div className="flex items-center justify-end bg-[color-mix(in_srgb,var(--c-s0)_70%,var(--c-base))] px-[10px] font-medium">{formatDuration(section.flatSub)}</div>
      )}
    </div>
  );
}

function WeekRow({ section, phone }: { section: WeekSection; phone: boolean }) {
  return (
    <div data-testid="week-row" className="grid h-8 grid-cols-2 border-b border-s1 text-[12px] tabular-nums shadow-[inset_0_1px_0_var(--c-s1)] max-md:grid-cols-1">
      <div className="flex items-center gap-2 px-[10px]">
        <span className="tt-label text-sub1">{section.label}</span>
        <span className="text-[11px] text-faint">{section.range}</span>
        <span className="ml-auto font-medium">
          {phone ? (
            <>
              <span className="text-faint">{formatDuration(section.rawSub)} → </span>
              {formatDuration(section.flatSub)}
            </>
          ) : (
            formatDuration(section.rawSub)
          )}
        </span>
      </div>
      {!phone && <div className="flex items-center justify-end px-[10px] font-medium">{formatDuration(section.flatSub)}</div>}
    </div>
  );
}

function Badges({ badges }: { badges: Badge[] }) {
  return (
    <>
      {badges.map((b) => (
        <span key={b} data-testid="badge" className={cn("w-max rounded-sm px-[6px] py-px text-[10px]", b === "moved" ? "text-peach-fg" : "text-muted")} style={{ background: BADGE_STYLE[b] }}>
          {b}
        </span>
      ))}
    </>
  );
}

function Pair({
  row,
  tz,
  title,
  color,
  group,
  hovered,
  onHover,
}: {
  row: PairRow;
  tz: string;
  title: (task: string) => string;
  color: (task: string) => string;
  group: RoundGroup;
  hovered: boolean;
  onHover: (on: boolean) => void;
}) {
  const { raw, flat } = row;
  const handlers = { onMouseEnter: () => onHover(true), onMouseLeave: () => onHover(false), onFocus: () => onHover(true), onBlur: () => onHover(false) };
  return (
    <div
      data-testid="pair-row"
      data-hovered={hovered || undefined}
      className={cn("grid h-8 grid-cols-2 border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] text-[12px] tabular-nums", hovered && "bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)]")}
    >
      <div tabIndex={raw ? 0 : -1} {...handlers} data-testid="raw-row" data-start={raw?.start} data-end={raw?.end} className="grid min-w-0 grid-cols-[92px_minmax(0,1fr)_52px] items-center gap-[10px] px-[10px] outline-none">
        {raw && <RawCells raw={raw} tz={tz} title={title(raw.task)} color={color(raw.task)} />}
      </div>
      <div
        tabIndex={flat ? 0 : -1}
        {...handlers}
        data-testid="flat-row"
        data-start={flat?.start}
        data-end={flat?.end}
        data-badge={flat?.badges.join(" ") || undefined}
        className={cn(
          "grid min-w-0 items-center gap-[10px] border-l border-accent px-[10px] outline-none",
          group === "entry" ? "grid-cols-[92px_auto_minmax(0,1fr)_52px]" : "grid-cols-[minmax(0,1fr)_auto_auto_52px]",
          flat?.dropped && "opacity-50",
        )}
      >
        {flat &&
          (group === "entry" ? (
            <>
              <span className="font-mono text-[11.5px]">
                {formatClock(tz, flat.start)}–{formatClock(tz, flat.end)}
              </span>
              <span className="flex">{flat.plusDay && <PlusDay />}</span>
              <span className="flex min-w-0 items-center gap-1 overflow-hidden">
                {hovered && raw && (
                  <span className="flex-none font-mono text-[10.5px] text-muted" data-testid="raw-reveal">
                    {formatClock(tz, raw.start)}–{formatClock(tz, raw.end)}
                  </span>
                )}
                <Badges badges={flat.badges} />
                {flat.dropped && <span className="whitespace-nowrap text-[10.5px] text-faint">not in download</span>}
              </span>
              <span className="text-right">{formatDuration(flat.seconds)}</span>
            </>
          ) : (
            <TaskDayCells flat={flat} tz={tz} title={title(flat.task)} color={color(flat.task)} />
          ))}
      </div>
    </div>
  );
}

function RawCells({ raw, tz, title, color }: { raw: RawRow; tz: string; title: string; color: string }) {
  return (
    <>
      <span className="font-mono text-[11.5px] text-sub1">
        {formatClock(tz, raw.start)}–{formatClock(tz, raw.end)}
      </span>
      <span className="flex min-w-0 items-center gap-[6px]">
        <span className="size-[7px] shrink-0 rounded-xs" style={{ background: color }} />
        <span className="truncate">{title}</span>
      </span>
      <span className="text-right">{formatDuration(raw.seconds)}</span>
    </>
  );
}

function TaskDayCells({ flat, tz, title, color }: { flat: FlatRow; tz: string; title: string; color: string }) {
  return (
    <>
      <span className="flex min-w-0 items-center gap-[6px]">
        <span className="size-[7px] shrink-0 rounded-xs" style={{ background: color }} />
        <span className="truncate">{title}</span>
      </span>
      <span className="whitespace-nowrap text-[11px] text-faint">
        {flat.count} {flat.count === 1 ? "entry" : "entries"} · {formatClock(tz, flat.start)}–{formatClock(tz, flat.end)}
        {flat.plusDay && <PlusDay />}
      </span>
      <span className="flex gap-1">
        <Badges badges={flat.badges} />
      </span>
      <span className="text-right">{formatDuration(flat.seconds)}</span>
    </>
  );
}

function PlusDay() {
  return (
    <span title="Pushed past midnight" className="rounded-xs bg-s0 px-1 font-mono text-[9.5px] leading-[14px] text-sub1">
      +1d
    </span>
  );
}

/** Phone (9.6): one stacked row, the raw time faint above the rounded time. */
function PhoneRow({ row, tz, title, color }: { row: PairRow; tz: string; title: (task: string) => string; color: (task: string) => string }) {
  const { raw, flat } = row;
  const task = flat?.task ?? raw!.task;
  return (
    <div data-testid="phone-row" className={cn("flex items-center gap-[10px] border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] px-3 py-2", flat?.dropped && "opacity-50")}>
      <span className="size-2 flex-none rounded-xs" style={{ background: color(task) }} />
      <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="truncate text-[13px]">{title(task)}</span>
        <span className="flex min-h-4 gap-1">
          {flat && <Badges badges={flat.badges} />}
          {flat?.dropped && <span className="text-[10.5px] text-faint">not in download</span>}
        </span>
      </div>
      <div className="flex flex-none flex-col items-end gap-px font-mono tabular-nums">
        {raw && (
          <span className="text-[10.5px] text-faint" data-testid="phone-raw">
            {formatClock(tz, raw.start)}–{formatClock(tz, raw.end)}
          </span>
        )}
        {flat && (
          <span className="flex items-center gap-1 text-[12.5px]" data-testid="phone-flat">
            {flat.plusDay && <PlusDay />}
            {formatClock(tz, flat.start)}–{formatClock(tz, flat.end)}
          </span>
        )}
      </div>
      <span className="w-[50px] flex-none text-right text-[12.5px] tabular-nums">{flat ? formatDuration(flat.seconds) : ""}</span>
    </div>
  );
}

function Strips({
  tz,
  section,
  days,
  onDay,
  color,
  gridMinutes,
  mode,
  startGridMinutes,
}: {
  tz: string;
  section: DaySection;
  days: string[];
  onDay: (day: string) => void;
  color: (task: string) => string;
  gridMinutes: number;
  mode: "up" | "nearest";
  startGridMinutes: number;
}) {
  const raw = section.rows.flatMap((r) => (r.raw ? [r.raw] : []));
  const flat = section.rows.flatMap((r) => (r.flat ? [r.flat] : []));
  const all = [...raw, ...flat];
  if (!all.length) return null;
  const from = Math.min(...all.map((r) => r.start));
  const to = Math.max(...all.map((r) => r.end));
  const hour = 3_600_000;
  // Whole hours around the data.
  const fromHour = from - (from % hour);
  const toHour = to % hour ? to - (to % hour) + hour : to;
  const span = Math.max(toHour - fromHour, hour);
  const pos = (r: { start: number; end: number }) => ({ left: `${((r.start - fromHour) / span) * 100}%`, width: `${Math.max(((r.end - r.start) / span) * 100, 0.3)}%` });
  const rawEnd = Math.max(...raw.map((r) => r.end));
  const flatEnd = Math.max(...flat.filter((r) => !r.dropped).map((r) => r.end), rawEnd);
  const index = days.indexOf(section.day);
  return (
    <div className="flex flex-col gap-[6px] px-[18px] pt-3 text-[11px] text-muted" data-testid="strips">
      <div className="flex items-center gap-2">
        {days.length > 1 && (
          <span className="flex gap-[2px]">
            <button type="button" aria-label="Previous day" className="tt-icon-btn size-5" disabled={index <= 0} onClick={() => onDay(days[index - 1]!)}>
              <CaretLeft size={10} />
            </button>
            <button type="button" aria-label="Next day" className="tt-icon-btn size-5" disabled={index >= days.length - 1} onClick={() => onDay(days[index + 1]!)}>
              <CaretRight size={10} />
            </button>
          </span>
        )}
        <span className="font-medium text-fg" data-testid="strip-day">
          {formatDayShort(parseDate(section.day)!)}
        </span>
        <span>· raw</span>
        <span className="ml-auto tabular-nums">
          {formatClock(tz, fromHour)} → {formatClock(tz, toHour)}
        </span>
      </div>
      <div className="relative h-[14px] rounded-xs bg-base">
        {raw.map((r) => (
          <span key={`${r.key}-${r.start}`} className="absolute inset-y-0 rounded-xs opacity-80" style={{ ...pos(r), background: color(r.task) }} />
        ))}
      </div>
      <div className="mt-1 flex justify-between">
        <span>
          rounded {mode} to {gridMinutes} min{startGridMinutes ? `, starts snapped to ${startGridMinutes} min` : ""}, pushed apart
        </span>
        <span className="text-peach-fg">{flatEnd > rawEnd ? `day ends later: +${formatDuration((flatEnd - rawEnd) / 1000)}` : "same end"}</span>
      </div>
      <div className="relative h-[14px] rounded-xs bg-base">
        {flat
          .filter((r) => !r.dropped)
          .map((r) => (
            <span key={`${r.key}-${r.start}`} className="absolute inset-y-0 rounded-xs" style={{ ...pos(r), background: color(r.task), boxShadow: "inset 0 0 0 1px var(--c-base)" }} />
          ))}
      </div>
    </div>
  );
}
