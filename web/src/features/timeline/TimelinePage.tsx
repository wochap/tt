import { ArrowArcLeft, ArrowArcRight, CaretDown, CaretLeft, CaretRight, CaretUp } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";

import {
  addDays,
  addMonths,
  type CivilDate,
  type Entry,
  formatDate,
  formatDayLong,
  formatDayShort,
  formatDuration,
  formatSpan,
  isoWeek,
  MONTH_NAMES_LONG,
  monthStart,
  parseDate,
  sameDate,
  totals,
  weekStartDate,
  DAY_NAMES,
} from "@tt/domain";

import { Kbd } from "@/components/ui/kbd";
import { Segmented, SegmentedItem } from "@/components/ui/toggle-group";
import { Tooltip } from "@/components/ui/tooltip";
import { useActions } from "@/data/actions";
import { useNow, useSnapshot, useView } from "@/data/react";
import { entriesInRange, taskTotals } from "@/data/select";
import { projectColor, NEUTRAL } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { usePhone } from "@/lib/media";
import { type SnapMinutes, useSettings, useTz } from "@/lib/settings";
import { MOD, cn } from "@/lib/utils";

import { useStatusHints } from "../shell/ui-state.tsx";
import { EmptyDay, EmptyMonth, EmptyWeek } from "./EmptyStates.tsx";
import { EntrySheet } from "./EntrySheet.tsx";
import { type Column, columnsFrom, todayIn } from "./geometry.ts";
import { JumpToDate } from "./JumpToDate.tsx";
import { MonthView, monthCells, monthSummary } from "./MonthView.tsx";
import { SnapMenu } from "./SnapMenu.tsx";
import { type PendingCreate, TimelineBoard } from "./TimelineBoard.tsx";
import { TotalsInline } from "./TotalsBar.tsx";

type RangeKind = "day" | "week" | "month";

export function TimelinePage() {
  const params = useParams();
  const [search, setSearch] = useSearchParams();
  const navigate = useNavigate();
  const phone = usePhone();
  const kind: RangeKind = phone ? "day" : params.range === "week" || params.range === "month" ? params.range : "day";
  const tz = useTz();
  const settings = useSettings();
  const now = useNow(1000);
  const nowLine = useNow(30_000);
  const view = useView();
  const actions = useActions();
  const today = todayIn(tz, now);
  const date = parseDate(search.get("date") ?? "") ?? today;
  const entryParam = search.get("entry");
  const [snap, setSnap] = useState<SnapMinutes>(settings.snapMinutes);
  useEffect(() => setSnap(settings.snapMinutes), [settings.snapMinutes]);
  const [jumpOpen, setJumpOpen] = useState(search.get("jump") === "1");
  const [focusedId, setFocusedId] = useState<string>();
  const [pending, setPending] = useState<PendingCreate | null>(null);

  useEffect(() => {
    if (search.get("jump") === "1") {
      setJumpOpen(true);
      search.delete("jump");
      setSearch(search, { replace: true });
    }
  }, [search, setSearch]);

  const go = useCallback(
    (next: { kind?: RangeKind; date?: CivilDate; entry?: string | null }) => {
      const q = new URLSearchParams(search);
      const d = next.date ?? date;
      if (sameDate(d, today)) q.delete("date");
      else q.set("date", formatDate(d));
      if (next.entry !== undefined) {
        if (next.entry) q.set("entry", next.entry);
        else q.delete("entry");
      }
      const qs = q.toString();
      void navigate(`/timeline/${next.kind ?? kind}${qs ? `?${qs}` : ""}`);
    },
    [search, date, today, kind, navigate],
  );

  const columns: Column[] = useMemo(() => {
    if (kind === "week") return columnsFrom(tz, weekStartDate(date, settings.weekStart), 7);
    return columnsFrom(tz, date, 1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, tz, formatDate(date), settings.weekStart]);

  const monthFirst = monthStart(date);
  const range =
    kind === "month"
      ? { from: columnsFrom(tz, monthFirst, 1)[0]!.from, to: columnsFrom(tz, addMonths(monthFirst, 1), 1)[0]!.from }
      : { from: columns[0]!.from, to: columns[columns.length - 1]!.to };
  const monthGrid = useMemo(() => {
    if (kind !== "month") return range;
    const first = weekStartDate(monthFirst, settings.weekStart);
    return { from: columnsFrom(tz, first, 1)[0]!.from, to: columnsFrom(tz, addDays(first, 42), 1)[0]!.from };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, tz, monthFirst.year, monthFirst.month, settings.weekStart]);
  const entries = useMemo(
    () => entriesInRange(view, kind === "month" ? monthGrid : range, now),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [view, range.from, range.to, monthGrid.from, monthGrid.to, Math.floor(now / 60_000), kind],
  );

  const step = (direction: number) => {
    if (kind === "day") go({ date: addDays(date, direction) });
    else if (kind === "week") go({ date: addDays(date, 7 * direction) });
    else go({ date: addMonths(monthFirst, direction) });
  };

  const sorted = useMemo(() => [...entries].sort((a, b) => a.start - b.start), [entries]);
  const moveFocus = (delta: number) => {
    if (!sorted.length) return;
    const at = sorted.findIndex((e) => e.id === focusedId);
    const next = sorted[at < 0 ? (delta > 0 ? 0 : sorted.length - 1) : Math.min(sorted.length - 1, Math.max(0, at + delta))]!;
    setFocusedId(next.id);
    document.querySelector<HTMLElement>(`[data-entry-id="${next.id}"] [role=button]`)?.focus();
  };

  const startAtNow = () => {
    const column = columns.find((c) => now >= c.from && now < c.to) ?? columnsFrom(tz, today, 1)[0]!;
    if (!columns.some((c) => c.key === column.key)) go({ date: today });
    setPending({ column, start: now, end: null, rect: null });
  };

  useKeys({
    d: () => go({ kind: "day" }),
    w: () => go({ kind: "week" }),
    m: () => go({ kind: "month" }),
    "arrowleft": () => step(-1),
    "arrowright": () => step(1),
    t: () => go({ date: today, kind: kind === "month" ? "day" : kind }),
    g: () => setJumpOpen(true),
    ...(kind !== "month"
      ? {
          j: () => moveFocus(1),
          k: () => moveFocus(-1),
          enter: () => (focusedId ? go({ entry: focusedId }) : false),
          s: () => {
            const focused = focusedId ? view.entries.get(focusedId) : undefined;
            if (focused) actions.toggle(focused.task);
            else startAtNow();
          },
          n: () => startAtNow(),
          "alt+1": () => setSnap(5),
          "alt+2": () => setSnap(10),
          "alt+3": () => setSnap(15),
          "alt+0": () => setSnap(0),
        }
      : {}),
  });

  useStatusHints(
    kind === "day" ? (
      <>
        <span><Kbd>J</Kbd><Kbd>K</Kbd> move</span>
        <span><Kbd>←</Kbd><Kbd>→</Kbd> day</span>
        <span><Kbd>T</Kbd> today</span>
        <span><Kbd>S</Kbd> start</span>
        <span><Kbd>?</Kbd> all shortcuts</span>
      </>
    ) : kind === "week" ? (
      <>
        <span><Kbd>←</Kbd><Kbd>→</Kbd> week</span>
        <span><Kbd>D</Kbd> day</span>
        <span><Kbd>N</Kbd> new entry</span>
        <span><Kbd>?</Kbd> all shortcuts</span>
      </>
    ) : (
      <>
        <span><Kbd>H</Kbd><Kbd>J</Kbd><Kbd>K</Kbd><Kbd>L</Kbd> move</span>
        <span><Kbd>←</Kbd><Kbd>→</Kbd> month</span>
        <span><Kbd>?</Kbd> all shortcuts</span>
      </>
    ),
    [kind],
  );

  const todayKey = formatDate(today);
  const openEntry = (entry: Entry) => {
    setFocusedId(entry.id);
    go({ entry: entry.id });
  };
  const closeSheet = useCallback(() => go({ entry: null }), [go]);
  const isEmpty = entries.length === 0 && !pending;
  const hasTasks = view.workspace.tasks.size > 0;

  const title =
    kind === "day" ? (
      <>
        {formatDayLong(date)}{" "}
        <span className="text-[12px] font-normal text-faint">
          {date.year} · W{isoWeek(date).week} · <Kbd>G</Kbd> jump
        </span>
      </>
    ) : kind === "week" ? (
      <>
        {formatSpan(columns[0]!.date, columns[6]!.date)}{" "}
        <span className="text-[12px] font-normal text-faint">
          {columns[6]!.date.year} · W{isoWeek(columns[0]!.date).week}
        </span>
      </>
    ) : (
      <>
        {MONTH_NAMES_LONG[date.month - 1]} <span className="text-[12px] font-normal text-faint">{date.year}</span>
      </>
    );

  if (phone) {
    return (
      <PhoneDay
        date={date}
        today={today}
        columns={columns}
        entries={entries}
        now={now}
        nowLine={nowLine}
        snap={snap}
        range={range}
        onStep={step}
        onOpen={openEntry}
        pending={pending}
        setPending={setPending}
        entryParam={entryParam}
        closeSheet={closeSheet}
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-10 flex-none items-center gap-3 border-b px-[14px]">
        <Segmented label="Range" value={kind} onValueChange={(next: RangeKind) => go({ kind: next })}>
          <SegmentedItem value="day">
            Day<Kbd>D</Kbd>
          </SegmentedItem>
          <SegmentedItem value="week">
            Week<Kbd>W</Kbd>
          </SegmentedItem>
          <SegmentedItem value="month">
            Month<Kbd>M</Kbd>
          </SegmentedItem>
        </Segmented>
        <div className="flex items-center gap-[2px]">
          <button type="button" className="tt-icon-btn" aria-label={`Previous ${kind}`} onClick={() => step(-1)}>
            <CaretLeft size={12} />
          </button>
          <button type="button" className="tt-icon-btn mx-[2px] w-auto px-[9px] text-[12px] text-sub1" onClick={() => go({ date: today })}>
            {kind === "day" ? "Today" : kind === "week" ? "This week" : "This month"}
          </button>
          <button type="button" className="tt-icon-btn" aria-label={`Next ${kind}`} onClick={() => step(1)}>
            <CaretRight size={12} />
          </button>
        </div>
        <JumpToDate
          open={jumpOpen}
          onOpenChange={setJumpOpen}
          date={date}
          today={today}
          weekStart={settings.weekStart}
          onGo={(d, day) => go({ date: d, kind: day ? "day" : kind })}
        >
          {title}
        </JumpToDate>
        <div className="ml-auto flex items-center gap-2 text-[12px] text-muted">
          {kind === "month" ? (
            <span className="tt-chip-btn">
              Show <span className="text-fg">top {settings.monthTopTasks} tasks</span>
            </span>
          ) : (
            <SnapMenu value={snap} onChange={setSnap} hint={kind === "day"} />
          )}
          <HistoryButtons />
        </div>
      </div>

      {kind === "month" ? (
        <>
          <div className="relative flex min-h-0 flex-1 flex-col">
            {entries.filter((e) => e.start < range.to && (e.end ?? now) > range.from).length === 0 && (
              <EmptyMonth onToday={() => go({ kind: "day", date: today })} />
            )}
            <MonthView
              view={view}
              tz={tz}
              month={monthFirst}
              today={today}
              weekStart={settings.weekStart}
              now={now}
              entries={entries}
              topTasks={settings.monthTopTasks}
              onOpenDay={(d) => go({ kind: "day", date: d })}
            />
          </div>
          <MonthFooter monthFirst={monthFirst} entries={entries} now={now} />
        </>
      ) : (
        <>
          <TimelineBoard
            columns={columns}
            density={kind}
            pxPerHour={kind === "day" ? 56 : 52}
            snapMinutes={snap}
            entries={entries}
            now={now}
            nowLine={nowLine}
            todayKey={todayKey}
            selectedId={entryParam ?? undefined}
            focusedId={focusedId}
            onOpen={openEntry}
            side={kind === "day" ? { width: 292, title: sameDate(date, today) ? "Today" : formatDayShort(date), showToday: true } : { width: 228, title: "This week", showToday: false }}
            header={kind === "week" ? <WeekHeader columns={columns} entries={entries} now={now} todayKey={todayKey} onOpenDay={(d) => go({ kind: "day", date: d })} /> : undefined}
            emptyOverlay={isEmpty ? kind === "day" ? <EmptyDay hasTasks={hasTasks} onStart={startAtNow} /> : <EmptyWeek /> : undefined}
            pendingCreate={pending}
            setPendingCreate={setPending}
            snapLabel={snap ? "snapped" : undefined}
          />
          {kind === "week" && <WeekFooter columns={columns} entries={entries} now={now} range={range} />}
        </>
      )}
      <EntrySheet entryId={entryParam} onClose={closeSheet} />
    </div>
  );
}

function HistoryButtons() {
  const snapshot = useSnapshot();
  const actions = useActions();
  return (
    <span className="flex">
      <Tooltip content={snapshot.undoLabel ? `Undo: ${snapshot.undoLabel} (${MOD}Z)` : `Undo (${MOD}Z)`}>
        <button
          type="button"
          aria-label="Undo"
          disabled={!snapshot.canUndo}
          onClick={() => void actions.undo()}
          className="tt-icon-btn rounded-r-none"
        >
          <ArrowArcLeft size={13} />
        </button>
      </Tooltip>
      <Tooltip content={snapshot.redoLabel ? `Redo: ${snapshot.redoLabel} (⇧${MOD}Z)` : `Redo (⇧${MOD}Z)`}>
        <button
          type="button"
          aria-label="Redo"
          disabled={!snapshot.canRedo}
          onClick={() => void actions.redo()}
          className="tt-icon-btn rounded-l-none border-l-0"
        >
          <ArrowArcRight size={13} />
        </button>
      </Tooltip>
    </span>
  );
}

function WeekHeader({ columns, entries, now, todayKey, onOpenDay }: { columns: Column[]; entries: Entry[]; now: number; todayKey: string; onOpenDay: (d: CivilDate) => void }) {
  return (
    <div className="grid h-12 flex-none items-end pb-[6px] pl-[14px] pr-[18px]" style={{ gridTemplateColumns: "52px repeat(7, minmax(0, 1fr))" }}>
      <span />
      {columns.map((column) => {
        const total = totals(entries, now, { from: column.from, to: column.to }).summed;
        const isToday = column.key === todayKey;
        const future = column.key > todayKey;
        const weekday = DAY_NAMES[(new Date(Date.UTC(column.date.year, column.date.month - 1, column.date.day)).getUTCDay() + 6) % 7];
        return (
          <button
            type="button"
            key={column.key}
            onClick={() => onOpenDay(column.date)}
            className="flex items-baseline justify-between border-l px-[6px] text-left"
            data-testid="week-day-header"
          >
            <span className="flex items-baseline gap-[5px]">
              <span className="text-[11px] uppercase tracking-[.06em] text-muted">{weekday}</span>
              <span
                className={cn(
                  "text-[15px] font-medium",
                  isToday ? "rounded-2 bg-accent px-[5px] leading-5 text-base" : future ? "text-faint" : "text-fg",
                )}
              >
                {column.date.day}
              </span>
            </span>
            <span className="text-[11px] tabular-nums text-faint">{total > 0 ? formatDuration(total) : ""}</span>
          </button>
        );
      })}
    </div>
  );
}

function WeekFooter({ columns, entries, now, range }: { columns: Column[]; entries: Entry[]; now: number; range: { from: number; to: number } }) {
  const view = useView();
  const days = columns.filter((c) => totals(entries, now, { from: c.from, to: c.to }).summed > 0).length;
  const byProject = new Map<string, number>();
  for (const [task, seconds] of taskTotals({ ...view, entries: new Map(entries.map((e) => [e.id, e])) }, now, range)) {
    const project = view.workspace.tasks.get(task)?.project;
    const key = project && view.workspace.projects.has(project) ? project : "";
    byProject.set(key, (byProject.get(key) ?? 0) + seconds);
  }
  const legend = [...byProject.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5);
  return (
    <div className="flex h-8 flex-none items-center gap-[22px] border-t px-[14px] text-[12px] tabular-nums">
      <span className="text-muted">Week · {days} days tracked</span>
      <TotalsInline entries={entries} now={now} range={range} />
      <span className="ml-auto flex gap-[14px] text-muted">
        {legend.map(([project, seconds]) => {
          const record = project ? view.workspace.projects.get(project) : undefined;
          return (
            <span key={project || "none"} className="flex items-center gap-[5px]">
              <span className="size-2 rounded-xs" style={{ background: record ? projectColor(record) : NEUTRAL }} />
              {record?.name ?? "no project"} {formatDuration(seconds)}
            </span>
          );
        })}
      </span>
    </div>
  );
}

function MonthFooter({ monthFirst, entries, now }: { monthFirst: CivilDate; entries: Entry[]; now: number }) {
  const tz = useTz();
  const { weekStart } = useSettings();
  const summary = monthSummary(monthCells(tz, monthFirst, weekStart, now, entries));
  return (
    <div className="flex h-8 flex-none items-center gap-[22px] border-t px-[14px] text-[12px] tabular-nums">
      <span className="text-muted">
        {MONTH_NAMES_LONG[monthFirst.month - 1]} · {summary.days} days
      </span>
      <span>
        <span className="text-muted">Summed </span>
        <span className="font-medium">{formatDuration(summary.summed)}</span>
      </span>
      <span>
        <span className="text-muted">Wall-clock </span>
        <span className="font-medium">{formatDuration(summary.wall)}</span>
      </span>
      <span>
        <span className="text-muted">Avg / day </span>
        <span className="font-medium">{formatDuration(summary.avg)}</span>
      </span>
      <span className="ml-auto text-faint">
        Click a day or press <Kbd>Enter</Kbd> to open it
      </span>
    </div>
  );
}

/** Frame 11: day view at phone width with a collapsible totals footer. */
function PhoneDay(props: {
  date: CivilDate;
  today: CivilDate;
  columns: Column[];
  entries: Entry[];
  now: number;
  nowLine: number;
  snap: SnapMinutes;
  range: { from: number; to: number };
  onStep: (direction: number) => void;
  onOpen: (entry: Entry) => void;
  pending: PendingCreate | null;
  setPending: (pending: PendingCreate | null) => void;
  entryParam: string | null;
  closeSheet: () => void;
}) {
  const [open, setOpen] = useState(false);
  const t = totals(props.entries, props.now, props.range);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-11 flex-none items-center gap-2 border-b px-[14px]">
        <button type="button" aria-label="Previous day" className="tt-icon-btn size-8" onClick={() => props.onStep(-1)}>
          <CaretLeft size={13} />
        </button>
        <span className="flex-1 text-center text-[14px] font-medium">
          {formatDayShort(props.date)} <span className="text-[11px] font-normal text-faint">· {formatDuration(t.summed)}</span>
        </span>
        <button type="button" aria-label="Next day" className="tt-icon-btn size-8" onClick={() => props.onStep(1)}>
          <CaretRight size={13} />
        </button>
      </div>
      <TimelineBoard
        columns={props.columns}
        density="phone"
        pxPerHour={44}
        snapMinutes={props.snap}
        entries={props.entries}
        now={props.now}
        nowLine={props.nowLine}
        todayKey={formatDate(props.today)}
        selectedId={props.entryParam ?? undefined}
        onOpen={props.onOpen}
        pendingCreate={props.pending}
        setPendingCreate={props.setPending}
      />
      <div className="flex flex-none flex-col gap-[6px] border-t bg-mantle px-[14px] py-2 text-[12px] tabular-nums" data-testid="phone-totals">
        <button type="button" className="flex min-h-7 items-center justify-between" onClick={() => setOpen(!open)} aria-expanded={open}>
          <span className="tt-label">Today · {t.count} entries</span>
          <span className="flex items-center gap-2 text-muted">
            <span className="font-medium text-fg">{formatDuration(t.summed)}</span>
            <span className="flex size-7 items-center justify-center rounded-2 border border-s1">{open ? <CaretDown size={11} /> : <CaretUp size={11} />}</span>
          </span>
        </button>
        {open && (
          <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-[2px]">
            <span className="text-muted">Summed</span>
            <span>{formatDuration(t.summed)}</span>
            <span className="text-muted">Wall-clock</span>
            <span>{formatDuration(t.wall)}</span>
            <span className="text-muted">Overlap</span>
            <span className="text-peach-fg">{formatDuration(t.overlap)}</span>
          </div>
        )}
      </div>
      <EntrySheet entryId={props.entryParam} onClose={props.closeSheet} />
    </div>
  );
}
