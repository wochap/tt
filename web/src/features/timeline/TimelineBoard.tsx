import {
  type DragEndEvent,
  type DragMoveEvent,
  type DragOverEvent,
  type DragStartEvent,
  DndContext,
  PointerSensor,
  pointerWithin,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { type ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { type Entry, formatClock, formatDuration, type Uuid } from "@tt/domain";

import { TaskPicker } from "@/components/TaskPicker";
import { useActions } from "@/data/actions";
import { useView } from "@/data/react";
import { usePhone } from "@/lib/media";
import { useSettings, useTz } from "@/lib/settings";

import { type DragOrigin, type GridMetrics, type Pointer, type Preview, preview as computePreview } from "./drag.ts";
import { type Column, renderedHours } from "./geometry.ts";
import { ScrollFade, useMoreBelow } from "./ScrollFade.tsx";
import { SidePanel } from "./SidePanel.tsx";
import { TimelineGrid } from "./TimelineGrid.tsx";

export interface PendingCreate {
  column: Column;
  start: number;
  /** `null`: the N key's "new entry at now" (stays running). */
  end: number | null;
  rect: DOMRect | null;
}

export interface TimelineBoardProps {
  columns: Column[];
  density: "day" | "week" | "phone";
  pxPerHour: number;
  snapMinutes: number;
  entries: Entry[];
  now: number;
  nowLine: number;
  todayKey: string;
  selectedId?: string;
  focusedId?: string;
  onOpen: (entry: Entry) => void;
  side?: { width: number; title: string; showToday: boolean };
  header?: ReactNode;
  emptyOverlay?: ReactNode;
  pendingCreate: PendingCreate | null;
  setPendingCreate: (pending: PendingCreate | null) => void;
  snapLabel?: string;
}

/** DndContext around the grid and the side panel: create, move, resize, re-link. */
export function TimelineBoard(props: TimelineBoardProps) {
  const { columns, density, pxPerHour, snapMinutes, entries, now } = props;
  const view = useView();
  const actions = useActions();
  const tz = useTz();
  const settings = useSettings();
  const phone = usePhone();
  const hours = useMemo(
    () => renderedHours(tz, columns, entries, settings.visibleHours, now),
    // hour range only depends on minute-level changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tz, columns, entries, settings.visibleHours, Math.floor(now / 60_000)],
  );
  const columnEls = useRef(new Map<string, HTMLDivElement>());
  const ghostEl = useRef<HTMLDivElement | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const moreBelow = useMoreBelow(scroller, density === "phone");
  const origin = useRef<DragOrigin | null>(null);
  const pointer = useRef<Pointer>({ x: 0, y: 0, alt: false, shift: false });
  const overTask = useRef<Uuid | undefined>(undefined);
  const justDragged = useRef(0);
  const [preview, setPreview] = useState<Preview | null>(null);

  const metrics = useCallback(
    (): GridMetrics => ({
      tz,
      columns,
      firstHour: hours[0],
      pxPerHour,
      rects: () => columns.map((c) => columnEls.current.get(c.key)?.getBoundingClientRect() ?? new DOMRect()),
      snapMinutes,
      now: Date.now(),
    }),
    [tz, columns, hours, pxPerHour, snapMinutes],
  );

  // Track pointer and modifiers during a drag (dnd-kit reports only deltas).
  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      pointer.current = { x: e.clientX, y: e.clientY, alt: e.altKey, shift: e.shiftKey };
    };
    const onKey = (e: KeyboardEvent) => {
      pointer.current = { ...pointer.current, alt: e.altKey, shift: e.shiftKey };
      if (origin.current) setPreview(computePreview(origin.current, pointer.current, metrics(), overTask.current));
    };
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKey, true);
    return () => {
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKey, true);
    };
  }, [metrics]);

  // Scroll the now line (or the first entry) into view once.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const line = el.querySelector<HTMLElement>("[data-testid=now-line]");
    const first = el.querySelector<HTMLElement>("[data-testid=entry-block]");
    const target = line ?? first;
    if (target && el.scrollHeight > el.clientHeight) {
      const top = target.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
      el.scrollTop = Math.max(0, top - el.clientHeight / 3);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [columns[0]?.key, density]);

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  const onDragStart = (event: DragStartEvent) => {
    const data = event.active.data.current as { kind: DragOrigin["kind"]; entry?: Entry; column?: Column };
    const start = event.activatorEvent as PointerEvent;
    const startPointer = { x: start.clientX, y: start.clientY, alt: start.altKey, shift: start.shiftKey };
    let column = data.column;
    if (!column) {
      // Entry drags: the column the block was grabbed in.
      const key = (start.target as HTMLElement).closest<HTMLElement>("[data-column]")?.dataset.column;
      column = columns.find((c) => c.key === key) ?? columns[0];
    }
    if (!column) return;
    props.setPendingCreate(null);
    origin.current = { kind: data.kind, entry: data.entry, column, pointer: startPointer };
    pointer.current = startPointer;
    overTask.current = undefined;
    setPreview(computePreview(origin.current, startPointer, metrics()));
  };

  const onDragMove = (_event: DragMoveEvent) => {
    if (origin.current) setPreview(computePreview(origin.current, pointer.current, metrics(), overTask.current));
  };

  const onDragOver = (event: DragOverEvent) => {
    const over = event.over?.data.current as { task?: Uuid } | undefined;
    overTask.current = origin.current?.kind === "move" ? over?.task : undefined;
    if (origin.current) setPreview(computePreview(origin.current, pointer.current, metrics(), overTask.current));
  };

  const finish = () => {
    origin.current = null;
    overTask.current = undefined;
    justDragged.current = Date.now();
    setPreview(null);
  };

  const onDragEnd = (_event: DragEndEvent) => {
    const o = origin.current;
    if (!o) return finish();
    const result = computePreview(o, pointer.current, metrics(), overTask.current);
    finish();
    if (result.kind === "create") {
      // Below 5 min the ghost is discarded (a click, not a drag); the small
      // tolerance keeps an exact 5 min drag with ⌥ from rounding below it.
      if (result.rawMinutes < 4.5) return;
      // Open the picker anchored to the ghost (rendered from pendingCreate).
      props.setPendingCreate({ column: result.column, start: result.start, end: result.end, rect: null });
      return;
    }
    if (result.overTask) {
      actions.relink(result.entry.id, result.overTask);
      return;
    }
    const { entry } = result;
    if (result.start === entry.start && result.end === entry.end) return;
    if (result.kind === "move") actions.moveEntry(entry.id, result.start, result.end);
    else if (result.kind === "resize-start") {
      try {
        actions.updateEntry(entry.id, { start: result.start }, `Changed start to ${formatClock(tz, result.start)}`);
      } catch (error) {
        console.warn(error);
      }
    } else {
      try {
        actions.updateEntry(
          entry.id,
          { end: result.end },
          entry.end === null
            ? `Stopped at ${formatClock(tz, result.end!)}`
            : `Changed end to ${formatClock(tz, result.end!)} · ${formatDuration(((result.end ?? 0) - entry.start) / 1000)}`,
        );
      } catch (error) {
        console.warn(error);
      }
    }
  };

  // Anchor the picker to the ghost once it is rendered.
  const pending = props.pendingCreate;
  useLayoutEffect(() => {
    if (pending && !pending.rect && ghostEl.current) {
      props.setPendingCreate({ ...pending, rect: ghostEl.current.getBoundingClientRect() });
    }
  });

  const pickerOpen = !!pending?.rect;
  const canKeepRunning = !!pending && (pending.end === null || pending.end >= Date.now() - Math.max(snapMinutes, 1) * 60_000);
  const close = () => props.setPendingCreate(null);
  const commitPick = (taskId: Uuid, keepRunning: boolean) => {
    if (!pending) return;
    if (pending.end === null) actions.start(taskId, pending.start);
    else actions.createEntry(taskId, pending.start, keepRunning ? null : pending.end);
    close();
  };

  const range = columns.length
    ? { from: columns[0]!.from, to: columns[columns.length - 1]!.to }
    : { from: now, to: now };

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={pointerWithin}
      autoScroll={{ threshold: { x: 0, y: 0.12 } }}
      onDragStart={onDragStart}
      onDragMove={onDragMove}
      onDragOver={onDragOver}
      onDragEnd={onDragEnd}
      onDragCancel={finish}
    >
      <div className="flex min-h-0 flex-1">
        <div
          className="flex min-w-0 flex-1 flex-col"
          onClickCapture={(event) => {
            // Swallow the click that ends a drag.
            if (Date.now() - justDragged.current < 250) event.stopPropagation();
          }}
        >
          {props.header}
          <div
            ref={scroller}
            className={density === "phone" ? "tt-scroll min-h-0 flex-1 overflow-y-auto pb-3 pl-[10px] pr-3 pt-[10px]" : "tt-scroll relative min-h-0 flex-1 overflow-y-auto pb-4 pl-[14px] pr-[18px] pt-[14px]"}
            data-testid="timeline-scroll"
          >
            <TimelineGrid
              view={view}
              tz={tz}
              columns={columns}
              entries={entries}
              now={now}
              nowLine={props.nowLine}
              hours={hours}
              pxPerHour={pxPerHour}
              density={density}
              todayKey={props.todayKey}
              preview={preview}
              pendingCreate={pending}
              selectedId={props.selectedId}
              focusedId={props.focusedId}
              onOpen={(entry) => {
                if (Date.now() - justDragged.current > 250) props.onOpen(entry);
              }}
              columnRef={(key, element) => {
                if (element) columnEls.current.set(key, element);
                else columnEls.current.delete(key);
              }}
              ghostRef={(element) => {
                ghostEl.current = element;
              }}
              emptyOverlay={props.emptyOverlay}
              snapLabel={props.snapLabel}
            />
            {density === "phone" && <ScrollFade show={moreBelow} />}
          </div>
        </div>
        {props.side && !phone && (
          <SidePanel
            width={props.side.width}
            title={props.side.title}
            showToday={props.side.showToday}
            range={range}
            entries={entries}
            now={now}
            overTask={preview && preview.kind !== "create" ? preview.overTask : undefined}
            dragging={!!preview}
          />
        )}
      </div>
      <TaskPicker
        open={pickerOpen}
        anchor={pending?.rect ?? null}
        canKeepRunning={canKeepRunning}
        headerHint={pending ? `${formatClock(tz, pending.start)} – ${pending.end === null ? "now" : formatClock(tz, pending.end)}` : undefined}
        onClose={close}
        onPick={(task, options) => commitPick(task.id, options.keepRunning)}
        onCreate={(input, options) => {
          const task = actions.createTask(input, { quiet: true });
          if (task) commitPick(task.id, options.keepRunning);
        }}
      />
    </DndContext>
  );
}
