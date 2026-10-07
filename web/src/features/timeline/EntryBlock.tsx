import { useDraggable } from "@dnd-kit/core";
import type { CSSProperties, KeyboardEvent, MouseEvent } from "react";

import { type Entry, entryDuration, formatClock, formatDuration, formatElapsed, laneGeometry, type Task } from "@tt/domain";

import { cn } from "@/lib/utils";

export interface EntryBlockProps {
  entry: Entry;
  task: Task | undefined;
  color: string;
  textColor: string;
  tz: string;
  now: number;
  /** Clipped to its column. */
  start: number;
  end: number;
  top: number;
  height: number;
  lane: number;
  lanes: number;
  /** "day" draws roomier blocks than "week". */
  density: "day" | "week" | "phone";
  selected?: boolean;
  focused?: boolean;
  dragging?: boolean;
  /** Delta badge while moving (`+30m`). */
  delta?: string;
  /** Live label while dragging. */
  dragLabel?: string;
  draggable?: boolean;
  onOpen?: (event: MouseEvent | KeyboardEvent) => void;
  clippedStart?: boolean;
  clippedEnd?: boolean;
}

/**
 * One entry on the grid: tinted by its project/tag color, with a draggable
 * body and 8 px resize handles. Compact (one line) below 44 px, or in week
 * view when lanes split the column. Running entries end at a dashed live edge.
 */
export function EntryBlock(props: EntryBlockProps) {
  const { entry, task, color, textColor, tz, now, top, height, lane, lanes, density, selected, focused, dragging, delta, dragLabel } = props;
  const running = entry.end === null;
  const compact = height < 44 || (density !== "day" && lanes > 1 && height < 64);
  const geometry = laneGeometry(lane, lanes);
  const style: CSSProperties = { position: "absolute", top, height, left: geometry.left, width: geometry.width, zIndex: dragging ? 30 : selected ? 6 : 5 };
  const day = density === "day";
  const range = dragLabel ?? `${formatClock(tz, props.start)} – ${running ? "now" : formatClock(tz, props.end)}`;
  const elapsed = formatElapsed(entryDuration(entry, now));
  const title = task?.title ?? "(deleted task)";

  const body = useDraggable({ id: `move:${entry.id}`, data: { kind: "move", entry }, disabled: !props.draggable });
  const topHandle = useDraggable({ id: `top:${entry.id}`, data: { kind: "resize-start", entry }, disabled: !props.draggable || props.clippedStart });
  const bottomHandle = useDraggable({ id: `bottom:${entry.id}`, data: { kind: "resize-end", entry }, disabled: !props.draggable || props.clippedEnd });

  return (
    <div
      style={style}
      className="group/entry"
      data-entry-id={entry.id}
      data-testid="entry-block"
      data-lane={lane}
      data-lanes={lanes}
      data-running={running || undefined}
    >
      <div
        ref={body.setNodeRef}
        {...body.attributes}
        {...body.listeners}
        role="button"
        tabIndex={focused ? 0 : -1}
        aria-label={`${title}, ${range}`}
        aria-pressed={selected}
        onClick={(event) => {
          event.stopPropagation();
          props.onOpen?.(event);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") props.onOpen?.(event);
        }}
        className={cn(
          "absolute inset-0 overflow-hidden outline-none transition-[background,box-shadow] duration-100",
          day ? "rounded-2 py-1 pl-[9px] pr-2" : "rounded-sm py-[3px] pl-[7px] pr-[6px]",
          props.draggable && (dragging ? "cursor-grabbing" : "cursor-grab"),
          focused && "outline-2 outline-offset-1 outline-accent",
        )}
        style={{
          background: `color-mix(in srgb, ${color} ${dragging ? 26 : 18}%, var(--c-base))`,
          borderLeft: `3px solid ${color}`,
          boxShadow: dragging
            ? "var(--shadow-lg)"
            : selected
              ? "0 0 0 1px var(--color-accent), 0 0 0 4px color-mix(in srgb, var(--color-accent) 25%, transparent)"
              : `inset 0 0 0 1px color-mix(in srgb, ${color} 25%, transparent)`,
        }}
      >
        {compact ? (
          <div className="flex h-full min-w-0 items-center gap-[6px]">
            <span className={cn("min-w-0 flex-1 truncate font-medium leading-[1.25]", day ? "text-[12.5px]" : "text-[11.5px]")}>{title}</span>
            {delta && <Delta text={delta} />}
            {running && (
              <>
                <span className="flex shrink-0 items-center gap-[5px] font-mono text-[9.5px]" style={{ color: textColor }}>
                  {elapsed}
                  {day && <PulseDot color={color} />}
                </span>
                <span className="pointer-events-none absolute inset-x-0 bottom-0 border-t-2 border-dashed" style={{ borderColor: color }} />
              </>
            )}
          </div>
        ) : (
          <div className="flex min-w-0 flex-col gap-[2px]">
            <div className="flex min-w-0 items-baseline gap-[6px]">
              <span className={cn("truncate font-medium", day ? "text-[12.5px]" : "text-[11.5px] leading-[1.25]")}>{title}</span>
              {delta && <Delta text={delta} />}
              {day && task && <span className="shrink-0 font-mono text-[10.5px] text-faint">#{task.seq}</span>}
            </div>
            <div className={cn("flex gap-2 tabular-nums text-muted", day ? "text-[11px]" : "text-[10.5px]")}>
              <span className="whitespace-nowrap">{range}</span>
              {day && !dragLabel && <span className="text-faint">{formatDuration(entryDuration(entry, now))}</span>}
              {day && entry.note && <span className="truncate text-faint">{entry.note}</span>}
            </div>
          </div>
        )}
        {running && !compact && (
          <div
            className={cn(
              "pointer-events-none absolute inset-x-0 bottom-0 flex items-center border-t border-dashed",
              day ? "h-4 justify-between pl-[9px] pr-2 text-[10px]" : "h-[14px] justify-end px-[6px] font-mono text-[9.5px]",
            )}
            style={{ borderColor: color, color: textColor, background: `color-mix(in srgb, ${color} 10%, transparent)` }}
          >
            {day && <span>running</span>}
            <span className="flex items-center gap-1 font-mono">
              {elapsed}
              {day && <PulseDot color={color} />}
            </span>
          </div>
        )}
      </div>
      {props.draggable && !props.clippedStart && (
        <div
          ref={topHandle.setNodeRef}
          {...topHandle.listeners}
          {...topHandle.attributes}
          tabIndex={-1}
          aria-label="Resize start"
          className="absolute inset-x-0 -top-[3px] h-2 cursor-ns-resize rounded-t-sm hover:bg-[color-mix(in_srgb,var(--entry-color)_45%,transparent)]"
          style={{ "--entry-color": color } as CSSProperties}
        />
      )}
      {props.draggable && !props.clippedEnd && (
        <div
          ref={bottomHandle.setNodeRef}
          {...bottomHandle.listeners}
          {...bottomHandle.attributes}
          tabIndex={-1}
          aria-label={running ? "Drag to stop" : "Resize end"}
          className="absolute inset-x-0 -bottom-[3px] h-2 cursor-ns-resize rounded-b-sm hover:bg-[color-mix(in_srgb,var(--entry-color)_45%,transparent)]"
          style={{ "--entry-color": color } as CSSProperties}
        />
      )}
    </div>
  );
}

function PulseDot({ color }: { color: string }) {
  return (
    <span
      className="size-[6px] shrink-0 animate-pulse-dot rounded-full"
      style={{ background: color, boxShadow: `0 0 0 3px color-mix(in srgb, ${color} 30%, transparent)` }}
    />
  );
}

function Delta({ text }: { text: string }) {
  return <span className="shrink-0 font-mono text-[10px] text-peach-fg">{text}</span>;
}

/** Dashed outline left at the origin while a block is moved. */
export function OriginGhost({ top, height, lane, lanes, density }: { top: number; height: number; lane: number; lanes: number; density: "day" | "week" | "phone" }) {
  const g = laneGeometry(lane, lanes);
  return (
    <div
      aria-hidden
      className={cn("pointer-events-none absolute border border-dashed border-s2 opacity-70", density === "day" ? "rounded-2" : "rounded-sm")}
      style={{ top, height, left: g.left, width: g.width }}
    />
  );
}
