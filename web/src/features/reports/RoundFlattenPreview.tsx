import { CaretLeft, CaretRight } from "@phosphor-icons/react";
import { useMemo, useState } from "react";

import {
  formatClock,
  formatDate,
  formatDayShort,
  formatDuration,
  localDate,
  type Millis,
  parseDate,
  type RoundGroup,
  type RoundInput,
  type Rounded,
  type View,
} from "@tt/domain";

import { taskColor } from "@/lib/colors";
import { cn } from "@/lib/utils";

export interface PreviewRow {
  /** Source entry of a raw row, or the first source entry of a rounded row. */
  key: string;
  task: string;
  start: Millis;
  end: Millis;
  seconds: number;
  badge?: "moved" | "rounded";
  count?: number;
}

/** Raw rows (clipped inputs) and flattened rows with moved/rounded badges. */
export function previewRows(inputs: readonly RoundInput[], rounded: readonly Rounded[]): { raw: PreviewRow[]; flat: PreviewRow[] } {
  const raw = inputs.map((item) => ({
    key: item.entries[0] ?? "",
    task: item.task,
    start: item.start,
    end: item.end,
    seconds: Math.max(0, Math.trunc((item.end - item.start) / 1000)),
  }));
  const firstStart = new Map<string, Millis>();
  for (const item of inputs) for (const id of item.entries) firstStart.set(id, item.start);
  const flat = rounded.map((item) => {
    const original = Math.min(...item.entries.map((id) => firstStart.get(id) ?? item.start));
    const moved = item.start > original;
    const badge: PreviewRow["badge"] = moved ? "moved" : item.duration !== item.original ? "rounded" : undefined;
    return { key: item.entries[0] ?? "", task: item.task, start: item.start, end: item.end, seconds: item.duration, badge, count: item.entries.length };
  });
  return { raw, flat };
}

/**
 * Frames 10 and 20: raw vs rounded & flattened, side by side, with strips for
 * one day. Every number comes from the shared `roundAndFlatten` output it is
 * given, so the preview equals the downloaded export.
 */
export function RoundFlattenPreview({
  view,
  tz,
  inputs,
  rounded,
  group,
  gridMinutes,
  mode,
}: {
  view: View;
  tz: string;
  inputs: readonly RoundInput[];
  rounded: readonly Rounded[];
  group: RoundGroup;
  gridMinutes: number;
  mode: "up" | "nearest";
}) {
  const { raw, flat } = useMemo(() => previewRows(inputs, rounded), [inputs, rounded]);
  const days = useMemo(() => [...new Set(raw.map((r) => formatDate(localDate(tz, r.start))))].sort(), [raw, tz]);
  const [picked, setPicked] = useState<string>();
  const day = picked && days.includes(picked) ? picked : days[days.length - 1];
  const rawTotal = raw.reduce((a, r) => a + r.seconds, 0);
  const flatTotal = flat.reduce((a, r) => a + r.seconds, 0);
  const title = (task: string) => view.workspace.tasks.get(task)?.title ?? "(deleted task)";
  const color = (task: string) => taskColor(view.workspace, view.workspace.tasks.get(task));

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="round-preview">
      {day && (
        <Strips
          tz={tz}
          day={day}
          days={days}
          onDay={setPicked}
          raw={raw.filter((r) => formatDate(localDate(tz, r.start)) === day)}
          flat={flat.filter((r) => formatDate(localDate(tz, r.start)) === day)}
          color={color}
          gridMinutes={gridMinutes}
          mode={mode}
        />
      )}
      <div className="grid min-h-0 flex-1 grid-cols-2 px-[18px] pt-[14px]">
        <div className="flex min-h-0 flex-col overflow-hidden rounded-l-md border">
          <div className="tt-label flex justify-between border-b px-[10px] py-[7px]">
            <span>Raw · {raw.length} entries</span>
            <span className="normal-case tracking-normal tabular-nums text-fg" data-testid="raw-total">
              {formatDuration(rawTotal)}
            </span>
          </div>
          <div className="tt-scroll min-h-0 flex-1 overflow-y-auto">
            {raw.map((row) => (
              <Row key={`${row.key}-${row.start}`} row={row} tz={tz} title={title(row.task)} color={color(row.task)} />
            ))}
          </div>
        </div>
        <div className="flex min-h-0 flex-col overflow-hidden rounded-r-md border border-accent bg-base">
          <div className="tt-label flex justify-between border-b px-[10px] py-[7px] text-link">
            <span>{group === "entry" ? "Rounded & flattened" : "Rounded · per task per day"}</span>
            <span className="normal-case tracking-normal tabular-nums text-fg" data-testid="flat-total">
              {formatDuration(flatTotal)}
            </span>
          </div>
          <div className="tt-scroll min-h-0 flex-1 overflow-y-auto" data-testid="flat-rows">
            {flat.map((row) =>
              group === "entry" ? (
                <Row key={`${row.key}-${row.start}`} row={row} tz={tz} title={title(row.task)} color={color(row.task)} badge />
              ) : (
                <div
                  key={`${row.key}-${row.start}`}
                  data-testid="flat-row"
                  data-start={row.start}
                  data-end={row.end}
                  className="grid h-[30px] grid-cols-[76px_1fr_90px_56px] items-center gap-[10px] border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] px-[10px] text-[12px] tabular-nums"
                >
                  <span className="text-sub1">{formatDayShort(parseDate(formatDate(localDate(tz, row.start)))!).replace(/^\w+ /, "")}</span>
                  <span className="flex min-w-0 items-center gap-[6px]">
                    <span className="size-[7px] shrink-0 rounded-xs" style={{ background: color(row.task) }} />
                    <span className="truncate">{title(row.task)}</span>
                  </span>
                  <span className="text-[11px] text-muted">
                    {row.count} {row.count === 1 ? "entry" : "entries"} · {formatClock(tz, row.start)}–{formatClock(tz, row.end)}
                  </span>
                  <span className="text-right">{formatDuration(row.seconds)}</span>
                </div>
              ),
            )}
          </div>
          {group === "task-day" && (
            <div className="px-[10px] py-2 text-[11px] text-muted">Same minutes, one row per task and day; each group starts at its first entry.</div>
          )}
        </div>
      </div>
    </div>
  );
}

function Row({ row, tz, title, color, badge }: { row: PreviewRow; tz: string; title: string; color: string; badge?: boolean }) {
  return (
    <div
      data-testid={badge ? "flat-row" : "raw-row"}
      data-start={row.start}
      data-end={row.end}
      className={cn(
        "grid h-8 items-center gap-[10px] border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] px-[10px] text-[12px] tabular-nums",
        badge ? "grid-cols-[96px_1fr_56px_56px]" : "grid-cols-[96px_1fr_56px]",
      )}
    >
      <span className="font-mono text-[11.5px] text-sub1">
        {formatClock(tz, row.start)}–{formatClock(tz, row.end)}
      </span>
      <span className="flex min-w-0 items-center gap-[6px]">
        <span className="size-[7px] shrink-0 rounded-xs" style={{ background: color }} />
        <span className="truncate">{title}</span>
      </span>
      {badge && (
        <span
          className={cn("w-max rounded-sm px-[6px] py-px text-[10px]", row.badge === "moved" ? "text-peach-fg" : "text-muted", !row.badge && "invisible")}
          style={row.badge === "moved" ? { background: "color-mix(in srgb, var(--ctp-peach) 18%, var(--c-base))" } : undefined}
        >
          {row.badge ?? "·"}
        </span>
      )}
      <span className="text-right">{formatDuration(row.seconds)}</span>
    </div>
  );
}

function Strips({
  tz,
  day,
  days,
  onDay,
  raw,
  flat,
  color,
  gridMinutes,
  mode,
}: {
  tz: string;
  day: string;
  days: string[];
  onDay: (day: string) => void;
  raw: PreviewRow[];
  flat: PreviewRow[];
  color: (task: string) => string;
  gridMinutes: number;
  mode: "up" | "nearest";
}) {
  const all = [...raw, ...flat];
  const from = Math.min(...all.map((r) => r.start));
  const to = Math.max(...all.map((r) => r.end));
  const hour = 3_600_000;
  // Whole local hours around the data.
  const fromHour = from - (from % hour);
  const toHour = to % hour ? to - (to % hour) + hour : to;
  const span = Math.max(toHour - fromHour, hour);
  const pos = (r: PreviewRow) => ({ left: `${((r.start - fromHour) / span) * 100}%`, width: `${Math.max(((r.end - r.start) / span) * 100, 0.3)}%` });
  const rawEnd = Math.max(...raw.map((r) => r.end));
  const flatEnd = Math.max(...flat.map((r) => r.end), rawEnd);
  const index = days.indexOf(day);
  return (
    <div className="flex flex-col gap-[6px] px-[18px] pt-[14px] text-[11px] text-muted" data-testid="strips">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1">
          {days.length > 1 && (
            <button type="button" aria-label="Previous day" className="tt-icon-btn size-5" disabled={index <= 0} onClick={() => onDay(days[index - 1]!)}>
              <CaretLeft size={10} />
            </button>
          )}
          {formatDayShort(parseDate(day)!)} · raw
          {days.length > 1 && (
            <button type="button" aria-label="Next day" className="tt-icon-btn size-5" disabled={index >= days.length - 1} onClick={() => onDay(days[index + 1]!)}>
              <CaretRight size={10} />
            </button>
          )}
        </span>
        <span>
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
          rounded {mode} to {gridMinutes} min, pushed apart
        </span>
        <span className="text-peach-fg">{flatEnd > rawEnd ? `day ends later: +${formatDuration((flatEnd - rawEnd) / 1000)}` : "same end"}</span>
      </div>
      <div className="relative h-[14px] rounded-xs bg-base">
        {flat.map((r) => (
          <span
            key={`${r.key}-${r.start}`}
            className="absolute inset-y-0 rounded-xs"
            style={{ ...pos(r), background: color(r.task), boxShadow: "inset 0 0 0 1px var(--c-base)" }}
          />
        ))}
      </div>
    </div>
  );
}
