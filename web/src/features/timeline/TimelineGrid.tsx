import { useDraggable } from "@dnd-kit/core";
import { type MouseEvent, type ReactNode, useMemo } from "react";

import { type Entry, formatClock, formatDuration, totals, type View } from "@tt/domain";

import { taskColor, taskTextColor } from "@/lib/colors";
import { cn } from "@/lib/utils";

import { type Preview, deltaLabel, rangeLabel } from "./drag.ts";
import { EntryBlock, OriginGhost } from "./EntryBlock.tsx";
import { type Column, type Placed, placeColumn, wallMinutes } from "./geometry.ts";
import { NowLine } from "./NowLine.tsx";

export interface TimelineGridProps {
  view: View;
  tz: string;
  columns: Column[];
  entries: Entry[];
  now: number;
  /** Re-rendered on a slower tick (now line). */
  nowLine: number;
  hours: [number, number];
  pxPerHour: number;
  density: "day" | "week" | "phone";
  todayKey: string;
  preview: Preview | null;
  /** Kept while the task picker is open after a drag-create. */
  pendingCreate: { column: Column; start: number; end: number | null } | null;
  selectedId?: string;
  focusedId?: string;
  onOpen: (entry: Entry, event: MouseEvent | KeyboardEvent) => void;
  columnRef: (key: string, element: HTMLDivElement | null) => void;
  ghostRef?: (element: HTMLDivElement | null) => void;
  /** Column headers (week view). */
  header?: ReactNode;
  emptyOverlay?: ReactNode;
  snapLabel?: string;
}

/** Hour gutter, day columns (dnd-kit "create" draggables), entries in lanes, now line. */
export function TimelineGrid(props: TimelineGridProps) {
  const { columns, entries, hours, pxPerHour, density, preview } = props;
  const [firstHour, lastHour] = hours;
  const height = (lastHour - firstHour) * pxPerHour;
  const gutter = density === "phone" ? 42 : 52;

  // Entries as drawn: the dragged one at its preview position.
  const drawn = useMemo(() => {
    if (!preview || preview.kind === "create" || preview.overTask) return entries;
    return entries.map((e) => (e.id === preview.entry.id ? { ...e, start: preview.start, end: preview.end } : e));
  }, [entries, preview]);

  const hourLabels = Array.from({ length: lastHour - firstHour }, (_, i) => firstHour + i);

  return (
    <div className="relative" style={{ height, paddingLeft: gutter }} data-first-hour={firstHour} data-px-per-hour={pxPerHour} data-testid="timeline-grid">
      {hourLabels.map((h, i) => (
        <div key={h} className="pointer-events-none absolute inset-x-0 border-t" style={{ top: i * pxPerHour, height: pxPerHour }}>
          <span
            className={cn(
              "absolute left-0 w-10 font-mono tabular-nums text-faint",
              density === "phone" ? "-top-[7px] w-[34px] text-[10px]" : "-top-2 text-[11px]",
            )}
          >
            {String(h).padStart(2, "0")}:00
          </span>
          {density === "day" && (
            <span
              className="absolute right-0 border-t border-dashed"
              style={{ left: gutter, top: pxPerHour / 2, borderColor: "color-mix(in srgb, var(--c-s0) 60%, transparent)" }}
            />
          )}
        </div>
      ))}
      <div className="absolute inset-y-0 right-0 grid" style={{ left: gutter, gridTemplateColumns: `repeat(${columns.length}, minmax(0, 1fr))` }}>
        {columns.map((column, index) => (
          <DayColumn
            key={column.key}
            {...props}
            column={column}
            index={index}
            entries={drawn}
            firstHour={firstHour}
            height={height}
          />
        ))}
      </div>
      {props.emptyOverlay}
    </div>
  );
}

function DayColumn(
  props: TimelineGridProps & { column: Column; index: number; firstHour: number; height: number },
) {
  const { tz, column, entries, now, firstHour, pxPerHour, density, preview, pendingCreate, todayKey, columns } = props;
  const create = useDraggable({ id: `create:${column.key}`, data: { kind: "create", column } });
  const placed = placeColumn(tz, column, entries, now, firstHour, pxPerHour);
  const isToday = column.key === todayKey;
  const yOf = (at: number) => ((wallMinutes(tz, column, at) - firstHour * 60) / 60) * pxPerHour;

  // Dashed outline at the origin while a block is moved.
  const origin = preview && preview.kind === "move" && !preview.overTask ? ghostFor(props, column, preview.entry) : null;

  const createBox =
    preview?.kind === "create" && preview.column.key === column.key
      ? { start: preview.start, end: preview.end, label: true }
      : pendingCreate && pendingCreate.column.key === column.key
        ? { start: pendingCreate.start, end: pendingCreate.end ?? now, label: false }
        : null;

  return (
    <div
      ref={(element) => {
        create.setNodeRef(element);
        props.columnRef(column.key, element);
      }}
      {...create.listeners}
      {...create.attributes}
      tabIndex={-1}
      role="presentation"
      data-column={column.key}
      data-testid="day-column"
      className={cn(
        "relative h-full cursor-crosshair outline-none",
        density !== "day" && "border-l px-[3px]",
        density === "week" && isToday && "bg-[color-mix(in_srgb,var(--color-accent)_4%,transparent)]",
      )}
      style={density === "week" && !isToday && isWeekend(column) ? weekendHatch : undefined}
    >
      <div className="relative h-full">
        {origin}
        {placed.map((p) => (
          <PlacedBlock key={p.entry.id} placed={p} {...props} />
        ))}
        {createBox && (
          <div
            ref={props.ghostRef}
            data-testid="create-ghost"
            className={cn(
              "pointer-events-none absolute inset-x-0 z-30 flex items-center gap-2 border border-dashed border-accent bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)] px-[9px] text-[11px] text-link",
              density === "day" ? "rounded-2" : "rounded-sm",
            )}
            style={{ top: yOf(createBox.start), height: Math.max(yOf(createBox.end) - yOf(createBox.start) - 2, 14) }}
          >
            <span className="whitespace-nowrap">
              {rangeLabel(tz, createBox.start, pendingCreate?.end === null && !createBox.label ? null : createBox.end)}
              {density !== "week" && ` · ${formatDuration((createBox.end - createBox.start) / 1000)}`}
            </span>
            {createBox.label && density === "day" && <span className="truncate text-faint">release to pick a task{props.snapLabel ? ` · ${props.snapLabel}` : ""}</span>}
          </div>
        )}
        {isToday && now >= column.from && now < column.to && (
          <NowLine top={yOf(props.nowLine || now)} tz={tz} now={props.nowLine || now} showLabel={columns.length === 1} />
        )}
      </div>
    </div>
  );
}

function ghostFor(props: TimelineGridProps & { firstHour: number }, column: Column, entry: Entry) {
  const original = props.view.entries.get(entry.id);
  if (!original) return null;
  const placed = placeColumn(props.tz, column, [original], props.now, props.firstHour, props.pxPerHour)[0];
  if (!placed) return null;
  return <OriginGhost top={placed.top} height={placed.height} lane={0} lanes={1} density={props.density} />;
}

function PlacedBlock({ placed, ...props }: TimelineGridProps & { placed: Placed }) {
  const { view, tz, now, density, preview } = props;
  const task = view.workspace.tasks.get(placed.entry.task);
  const active = preview && preview.kind !== "create" && preview.entry.id === placed.entry.id ? preview : null;
  const dragging = !!active;
  const relinking = !!active?.overTask;
  return (
    <EntryBlock
      entry={placed.entry}
      task={task}
      color={taskColor(view.workspace, task)}
      textColor={taskTextColor(view.workspace, task)}
      tz={tz}
      now={now}
      start={placed.start}
      end={placed.end}
      top={placed.top}
      height={placed.height}
      lane={placed.lane}
      lanes={placed.lanes}
      density={density}
      selected={props.selectedId === placed.entry.id}
      focused={props.focusedId === placed.entry.id}
      dragging={dragging && !relinking}
      delta={active ? deltaLabel(active.deltaMinutes) : undefined}
      dragLabel={
        active && !relinking
          ? `${formatClock(tz, active.start)} – ${active.end === null ? "now" : formatClock(tz, active.end)}${active.kind === "move" ? "" : ` · ${formatDuration(((active.end ?? now) - active.start) / 1000)}`}`
          : undefined
      }
      draggable
      clippedStart={placed.clippedStart}
      clippedEnd={placed.clippedEnd}
      onOpen={(event) => props.onOpen(placed.entry, event as MouseEvent)}
    />
  );
}

const weekendHatch = {
  background:
    "repeating-linear-gradient(135deg, transparent 0 6px, color-mix(in srgb, var(--c-s0) 50%, transparent) 6px 7px)",
};

function isWeekend(column: Column): boolean {
  const d = new Date(Date.UTC(column.date.year, column.date.month - 1, column.date.day)).getUTCDay();
  return d === 0 || d === 6;
}

/** Day totals for headers. */
export function columnTotal(entries: Entry[], column: Column, now: number): number {
  return totals(entries, now, { from: column.from, to: column.to }).summed;
}
