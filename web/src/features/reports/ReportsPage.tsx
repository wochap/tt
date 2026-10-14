import { CaretDown, CaretLeft, CaretRight, DownloadSimple } from "@phosphor-icons/react";
import { useMemo, useState } from "react";
import { useNavigate } from "react-router";

import {
  addDays,
  addMonths,
  type CivilDate,
  entriesCsv,
  entriesIn,
  exportJson,
  formatClock,
  formatDate,
  formatDayShort,
  formatDuration,
  formatSpan,
  localDate,
  localToUtc,
  MONTH_NAMES_LONG,
  monthStart,
  parseClock,
  type Range,
  report,
  type ReportGroup,
  type RoundGroup,
  type RoundMode,
  roundAndFlatten,
  roundedCsv,
  roundedJson,
  roundInputs,
  START_GRID_MINUTES,
  validStartGrids,
  type View,
  weekStartDate,
} from "@tt/domain";

import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Segmented, SegmentedItem } from "@/components/ui/toggle-group";
import { useNow, useView } from "@/data/react";
import { cssColor, NEUTRAL, projectColor, taskColor } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { useSettings, useTz } from "@/lib/settings";

import { useStatusHints } from "../shell/ui-state.tsx";
import { columnFor, todayIn } from "../timeline/geometry.ts";
import { RoundFlattenPreview } from "./RoundFlattenPreview.tsx";

type Preset = "day" | "week" | "month" | "custom";
type ExportMode = "json" | "csv" | "rounded";
const GRIDS = [5, 10, 15, 30, 60] as const;

/** Valid start grids (minutes, 0 = off) for a duration grid in minutes. */
export function validStartMinutes(gridMinutes: number): number[] {
  return validStartGrids(gridMinutes * 60).map((g) => g / 60);
}

/** Default start grid: the largest valid value ≤ 15 min. */
export function defaultStartMinutes(gridMinutes: number): number {
  return Math.max(...validStartMinutes(gridMinutes).filter((g) => g <= 15));
}

/** Keeps `start` when it divides the new duration grid, else the largest valid start. */
export function fallbackStartMinutes(gridMinutes: number, start: number): number {
  const valid = validStartMinutes(gridMinutes);
  return valid.includes(start) ? start : Math.max(...valid);
}

export interface RoundGrids {
  grid: number;
  mode: RoundMode;
  startGrid: number;
  startMode: RoundMode;
}

/** Duration grid + mode and start grid + mode; start grids that do not divide the duration are disabled. */
export function RoundGridControls({ value, onChange }: { value: RoundGrids; onChange: (value: RoundGrids) => void }) {
  const valid = validStartMinutes(value.grid);
  const modes: [string, string][] = [
    ["up", "up"],
    ["nearest", "nearest"],
  ];
  return (
    <>
      <Choice
        label="Duration"
        value={String(value.grid)}
        options={GRIDS.map((g) => [String(g), `${g} min`])}
        onChange={(v) => onChange({ ...value, grid: Number(v), startGrid: fallbackStartMinutes(Number(v), value.startGrid) })}
      />
      <Choice label="Round" value={value.mode} options={modes} onChange={(v) => onChange({ ...value, mode: v as RoundMode })} />
      <Choice
        label="Start"
        value={String(value.startGrid)}
        options={[0, ...START_GRID_MINUTES].map((g) => [String(g), g === 0 ? "off" : `${g} min`, !valid.includes(g)])}
        onChange={(v) => onChange({ ...value, startGrid: Number(v) })}
      />
      {value.startGrid > 0 && <Choice label="Start mode" value={value.startMode} options={modes} onChange={(v) => onChange({ ...value, startMode: v as RoundMode })} />}
    </>
  );
}

interface Custom {
  from: CivilDate;
  fromTime: string;
  to: CivilDate;
  toTime: string;
}

/** Resolves the custom range: empty times mean start / end of day. */
export function customRange(tz: string, custom: Custom): Range | undefined {
  const start = custom.fromTime.trim() ? parseClock(custom.fromTime.trim()) : { hour: 0, minute: 0, second: 0 };
  const end = custom.toTime.trim() ? parseClock(custom.toTime.trim()) : undefined;
  if (!start || (custom.toTime.trim() && !end)) return undefined;
  const from = localToUtc(tz, { ...custom.from, ...start });
  const to = end ? localToUtc(tz, { ...custom.to, ...end }) : columnFor(tz, addDays(custom.to, 1)).from;
  return to > from ? { from, to } : undefined;
}

export function ReportsPage() {
  const view = useView();
  const tz = useTz();
  const { weekStart, snapMinutes } = useSettings();
  const now = useNow(30_000);
  const navigate = useNavigate();
  const today = todayIn(tz, now);
  const [preset, setPreset] = useState<Preset>("week");
  const [anchor, setAnchor] = useState<CivilDate>(today);
  const [custom, setCustom] = useState<Custom>({ from: today, fromTime: "", to: today, toTime: "" });
  const [applied, setApplied] = useState<Custom>(custom);
  const [exportMode, setExportMode] = useState<ExportMode>("rounded");
  const [grids, setGrids] = useState<RoundGrids>(() => {
    const grid = GRIDS.includes(snapMinutes as (typeof GRIDS)[number]) ? snapMinutes : 15;
    return { grid, mode: "up", startGrid: defaultStartMinutes(grid), startMode: "up" };
  });
  const { grid, mode, startGrid, startMode } = grids;
  const [group, setGroup] = useState<RoundGroup>("entry");

  const range: Range | undefined = useMemo(() => {
    if (preset === "day") return { from: columnFor(tz, anchor).from, to: columnFor(tz, addDays(anchor, 1)).from };
    if (preset === "week") {
      const start = weekStartDate(anchor, weekStart);
      return { from: columnFor(tz, start).from, to: columnFor(tz, addDays(start, 7)).from };
    }
    if (preset === "month") {
      const start = monthStart(anchor);
      return { from: columnFor(tz, start).from, to: columnFor(tz, addMonths(start, 1)).from };
    }
    return customRange(tz, applied);
  }, [preset, anchor, tz, weekStart, applied]);

  const step = (direction: number) => {
    if (preset === "day") setAnchor(addDays(anchor, direction));
    else if (preset === "week") setAnchor(addDays(anchor, 7 * direction));
    else if (preset === "month") setAnchor(addMonths(anchor, direction));
  };
  useKeys({ arrowleft: () => step(-1), arrowright: () => step(1), d: () => setPreset("day"), w: () => setPreset("week"), m: () => setPreset("month") });
  useStatusHints(
    <>
      <span><Kbd>←</Kbd><Kbd>→</Kbd> range</span>
      <span><Kbd>D</Kbd><Kbd>W</Kbd><Kbd>M</Kbd> preset</span>
    </>,
    [],
  );

  const reports = useMemo(() => {
    if (!range) return undefined;
    return {
      task: report(view, range, "task", tz, now),
      tag: report(view, range, "tag", tz, now),
      project: report(view, range, "project", tz, now),
    };
  }, [view, range, tz, now]);
  const inputs = useMemo(() => (range ? roundInputs(view, range, now) : []), [view, range, now]);
  const options = useMemo(() => ({ gridSeconds: grid * 60, mode, startGridSeconds: startGrid * 60, startMode, group }), [grid, mode, startGrid, startMode, group]);
  const rounded = useMemo(() => roundAndFlatten(inputs, options, tz), [inputs, options, tz]);
  const hasData = !!reports && reports.task.count > 0;

  const title = (() => {
    if (!range) return "Invalid range";
    if (preset === "day") return `${formatDayShort(anchor)} ${anchor.year}`;
    if (preset === "week") {
      const start = weekStartDate(anchor, weekStart);
      return `${formatSpan(start, addDays(start, 6))} ${addDays(start, 6).year}`;
    }
    if (preset === "month") return `${MONTH_NAMES_LONG[anchor.month - 1]} ${anchor.year}`;
    return `${formatDayShort(localDate(tz, range.from))} ${formatClock(tz, range.from)} → ${formatDayShort(localDate(tz, range.to - 1))} ${formatClock(tz, range.to)}`;
  })();

  const roundedName = `${grid}m-start-${startGrid ? `${startGrid}m` : "off"}-${group}`;
  const download = () => {
    if (!range) return;
    const stamp = `${formatDate(localDate(tz, range.from))}_${formatDate(localDate(tz, range.to - 1))}`;
    if (exportMode === "json") save(`tt-export-${stamp}.json`, JSON.stringify(exportJson(view, range, Date.now()), null, 2), "application/json");
    else if (exportMode === "csv") save(`tt-entries-${stamp}.csv`, entriesCsv(view, entriesIn(view, range, now), now), "text/csv");
    else save(`tt-rounded-${roundedName}-${stamp}.csv`, roundedCsv(view, rounded), "text/csv");
  };
  const downloadRoundedJson = () => {
    if (!range) return;
    const stamp = `${formatDate(localDate(tz, range.from))}_${formatDate(localDate(tz, range.to - 1))}`;
    save(`tt-rounded-${roundedName}-${stamp}.json`, JSON.stringify(roundedJson(view, range, options, rounded), null, 2), "application/json");
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-10 flex-none flex-wrap items-center gap-3 border-b px-[14px] py-[6px] text-[12px]">
        <Segmented label="Range" value={preset} onValueChange={(p: Preset) => setPreset(p)}>
          {(["day", "week", "month", "custom"] as const).map((p) => (
            <SegmentedItem key={p} value={p}>
              {p[0]!.toUpperCase() + p.slice(1)}
            </SegmentedItem>
          ))}
        </Segmented>
        {preset !== "custom" ? (
          <>
            <div className="flex items-center gap-[2px]">
              <button type="button" className="tt-icon-btn" aria-label="Previous range" onClick={() => step(-1)}>
                <CaretLeft size={12} />
              </button>
              <button type="button" className="tt-icon-btn" aria-label="Next range" onClick={() => step(1)}>
                <CaretRight size={12} />
              </button>
            </div>
            <span className="text-[15px] font-medium" data-testid="report-title">
              {title}{" "}
              {range && (
                <span className="text-[12px] font-normal text-faint">
                  · {formatDayShort(localDate(tz, range.from)).split(" ")[0]} {formatClock(tz, range.from)} → {formatDayShort(localDate(tz, range.to - 1)).split(" ")[0]} 24:00
                </span>
              )}
            </span>
          </>
        ) : (
          <CustomRange custom={custom} setCustom={setCustom} today={today} weekStart={weekStart} onApply={() => setApplied(custom)} valid={!!customRange(tz, custom)} />
        )}
        {reports && (
          <span className="ml-auto flex gap-[18px] tabular-nums" data-testid="report-totals">
            <span>
              <span className="text-muted">Summed </span>
              <span className="font-medium">{formatDuration(reports.task.summed)}</span>
            </span>
            <span>
              <span className="text-muted">Wall-clock </span>
              <span className="font-medium">{formatDuration(reports.task.wall)}</span>
            </span>
            <span>
              <span className="text-muted">Entries </span>
              {reports.task.count}
            </span>
          </span>
        )}
      </div>

      <div className="flex min-h-0 flex-1 max-md:flex-col">
        <div className="tt-scroll flex w-[560px] flex-none flex-col gap-[18px] overflow-y-auto border-r px-[18px] py-4 max-md:w-full">
          {!hasData ? (
            <EmptyReport onToday={() => navigate("/timeline/day")} />
          ) : (
            <>
              <Bars title="By task" groups={reports!.task.groups} color={(g) => (g.key === "none" ? NEUTRAL : taskColor(view.workspace, view.workspace.tasks.get(g.key)))} label={(g) => (g.seq !== undefined ? `#${g.seq} ${g.label}` : g.label)} />
              <div className="grid grid-cols-2 gap-[18px]">
                <Bars small title="By tag" groups={reports!.tag.groups} color={(g) => cssColor(view.workspace.tags.get(g.key)?.color) ?? NEUTRAL} label={(g) => (g.key === "none" ? g.label : `+${g.label}`)} />
                <Bars small title="By project" groups={reports!.project.groups} color={(g) => projectColorOf(view, g.key)} label={(g) => (g.key === "none" ? "No project" : g.label)} />
              </div>
            </>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col bg-mantle">
          <div className="flex flex-wrap items-center gap-3 border-b px-[18px] py-3 text-[12px]">
            <span className="tt-label">Export</span>
            <Segmented label="Export format" value={exportMode} onValueChange={(m: ExportMode) => setExportMode(m)}>
              <SegmentedItem value="json" className="px-[10px] py-1">
                Raw JSON
              </SegmentedItem>
              <SegmentedItem value="csv" className="px-[10px] py-1">
                Raw CSV
              </SegmentedItem>
              <SegmentedItem value="rounded" className="px-[10px] py-1">
                Rounded &amp; flattened
              </SegmentedItem>
            </Segmented>
            {exportMode === "rounded" && (
              <>
                <RoundGridControls value={grids} onChange={setGrids} />
                <Segmented label="Grouping" value={group} onValueChange={(g: RoundGroup) => setGroup(g)}>
                  <SegmentedItem value="entry" className="px-[10px] py-1">
                    Per entry
                  </SegmentedItem>
                  <SegmentedItem value="task-day" className="px-[10px] py-1">
                    Per task / day
                  </SegmentedItem>
                </Segmented>
              </>
            )}
            <span className="ml-auto flex gap-2">
              {exportMode === "rounded" && (
                <Button variant="secondary" size="sm" disabled={!hasData} onClick={downloadRoundedJson}>
                  JSON
                </Button>
              )}
              <Button variant="primary" size="sm" className="gap-[6px]" disabled={!hasData} onClick={download} data-testid="download">
                <DownloadSimple size={12} /> Download {exportMode === "json" ? "JSON" : "CSV"}
              </Button>
            </span>
          </div>
          {!hasData || !range ? (
            <div className="m-[18px] flex flex-1 items-center justify-center rounded-md border border-dashed border-s2 p-3 text-center text-[12px] text-muted">
              Export is disabled
              <br />
              until there is data
            </div>
          ) : exportMode === "rounded" ? (
            <RoundFlattenPreview view={view} tz={tz} inputs={inputs} rounded={rounded} group={group} gridMinutes={grid} mode={mode} startGridMinutes={startGrid} />
          ) : (
            <pre className="tt-scroll m-[18px] min-h-0 flex-1 overflow-auto rounded-md border bg-base p-3 font-mono text-[11px] leading-[1.5] text-sub1" data-testid="export-preview">
              {exportMode === "json"
                ? JSON.stringify(exportJson(view, range, now), null, 2).slice(0, 20_000)
                : entriesCsv(view, entriesIn(view, range, now), now).slice(0, 20_000)}
            </pre>
          )}
          <div className="px-[18px] pb-3 pt-[10px] text-[11.5px] text-faint">
            Running entries are cut at export time. Overlap is resolved in start order; the later entry is pushed, never shortened.
          </div>
        </div>
      </div>
    </div>
  );
}

function projectColorOf(view: View, key: string): string {
  const project = view.workspace.projects.get(key);
  return project ? projectColor(project) : NEUTRAL;
}

function save(name: string, text: string, type: string) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function Bars({ title, groups, color, label, small }: { title: string; groups: ReportGroup[]; color: (g: ReportGroup) => string; label: (g: ReportGroup) => string; small?: boolean }) {
  const max = Math.max(1, ...groups.map((g) => g.summed));
  return (
    <section className="flex flex-col gap-[6px]" aria-label={title}>
      <div className="tt-label">{title}</div>
      {groups.slice(0, small ? 8 : 12).map((g) => (
        <div key={g.key} className={small ? "grid grid-cols-[1fr_60px] items-center gap-2 text-[12px] tabular-nums" : "grid grid-cols-[1fr_70px] items-center gap-3 text-[12.5px] tabular-nums"}>
          <div className="min-w-0">
            <div className={small ? "mb-[3px] truncate text-sub1" : "mb-[3px] truncate"}>{label(g)}</div>
            <div className="h-1 rounded-xs" style={{ width: `${Math.round((g.summed / max) * 100)}%`, background: color(g) }} />
          </div>
          <span className="text-right">{formatDuration(g.summed)}</span>
        </div>
      ))}
    </section>
  );
}

function Choice({ label, value, options, onChange }: { label: string; value: string; options: [string, string, boolean?][]; onChange: (v: string) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="tt-chip-btn text-sub1" aria-label={label}>
        {label} <span className="text-fg">{options.find(([v]) => v === value)?.[1]}</span>
        <CaretDown size={10} className="text-faint" />
      </DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuRadioGroup value={value} onValueChange={onChange}>
          {options.map(([v, text, disabled]) => (
            <DropdownMenuRadioItem key={v} value={v} disabled={disabled}>
              {text}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function CustomRange({
  custom,
  setCustom,
  today,
  weekStart,
  onApply,
  valid,
}: {
  custom: Custom;
  setCustom: (c: Custom) => void;
  today: CivilDate;
  weekStart: 0 | 1 | 2 | 3 | 4 | 5 | 6;
  onApply: () => void;
  valid: boolean;
}) {
  const time = (key: "fromTime" | "toTime", placeholder: string, dashed: boolean) => (
    <input
      aria-label={key === "fromTime" ? "From time" : "To time"}
      value={custom[key]}
      placeholder={placeholder}
      onChange={(e) => setCustom({ ...custom, [key]: e.target.value })}
      onKeyDown={(e) => e.key === "Enter" && valid && onApply()}
      onBlur={() => valid && onApply()}
      className={`h-7 w-[92px] rounded-2 border bg-mantle px-[10px] font-mono tabular-nums outline-none placeholder:text-muted focus:border-accent ${dashed ? "border-dashed border-s2" : "border-s1"}`}
    />
  );
  const date = (key: "from" | "to") => (
    <Popover>
      <PopoverTrigger className="flex h-7 items-center gap-2 rounded-2 border border-s1 bg-mantle px-[10px] tabular-nums" aria-label={key === "from" ? "From date" : "To date"}>
        {formatDayShort(custom[key])}
        <CaretDown size={10} className="text-faint" />
      </PopoverTrigger>
      <PopoverContent className="w-[260px] p-[10px]">
        <Calendar
          value={custom[key]}
          today={today}
          weekStart={weekStart}
          onSelect={(d) => {
            const next = { ...custom, [key]: d };
            if (key === "from" && formatDate(d) > formatDate(custom.to)) next.to = d;
            setCustom(next);
          }}
        />
      </PopoverContent>
    </Popover>
  );
  return (
    <span className="flex flex-wrap items-center gap-2" data-testid="custom-range">
      <span className="text-muted">From</span>
      {date("from")}
      {time("fromTime", "00:00", false)}
      <span className="text-muted">to</span>
      {date("to")}
      {time("toTime", "end of day", true)}
      <span className="text-[11.5px] text-muted">
        times optional · <Kbd>⏎</Kbd> applies
      </span>
      <Button size="sm" variant="secondary" disabled={!valid} onClick={onApply}>
        Apply
      </Button>
    </span>
  );
}

function EmptyReport({ onToday }: { onToday: () => void }) {
  return (
    <div className="flex flex-col gap-2" data-testid="empty-report">
      <div className="tt-label">By task</div>
      {[80, 55, 35].map((w) => (
        <div key={w} className="h-1 rounded-xs bg-s0" style={{ width: `${w}%` }} />
      ))}
      <div className="mt-[6px] text-[12px] leading-[1.5] text-muted">Nothing to report for this range. Totals, groupings and exports appear once an entry exists.</div>
      <button type="button" onClick={onToday} className="self-start text-[12.5px] text-link hover:underline">
        Go to today’s timeline →
      </button>
    </div>
  );
}
