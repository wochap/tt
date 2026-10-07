import { CaretDown, Stop, Play, X } from "@phosphor-icons/react";
import { type PointerEvent, type ReactNode, useEffect, useRef, useState } from "react";

import {
  DomainError,
  type Entry,
  entryDuration,
  formatClock,
  formatDayShort,
  formatElapsed,
  localDate,
  localToUtc,
  type Millis,
  parseClock,
  parseTime,
  taskProject,
} from "@tt/domain";

import { Seq, TagChips } from "@/components/task-bits";
import { TaskPicker } from "@/components/TaskPicker";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Sheet, SheetContent, SheetDescription, SheetTitle } from "@/components/ui/sheet";
import { useActions } from "@/data/actions";
import { useNow, useView } from "@/data/react";
import { taskColor } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { useSettings, useTz } from "@/lib/settings";
import { MOD, cn } from "@/lib/utils";

const MIN15 = 15 * 60_000;

/** Frame 4: non-modal sheet, every edit applies immediately and is undoable. */
export function EntrySheet({ entryId, onClose }: { entryId: string | null; onClose: () => void }) {
  const view = useView();
  const entry = entryId ? view.entries.get(entryId) : undefined;
  // Closing when the entry disappears (deleted here or elsewhere).
  useEffect(() => {
    if (entryId && !entry) onClose();
  }, [entryId, entry, onClose]);
  return (
    <Sheet open={!!entry} onOpenChange={(open) => !open && onClose()}>
      <SheetContent aria-describedby={undefined} data-testid="entry-sheet">
        {entry && <SheetBody key={entry.id} entry={entry} onClose={onClose} />}
      </SheetContent>
    </Sheet>
  );
}

function SheetBody({ entry, onClose }: { entry: Entry; onClose: () => void }) {
  const view = useView();
  const actions = useActions();
  const tz = useTz();
  const { snapMinutes } = useSettings();
  const now = useNow(1000);
  const task = view.workspace.tasks.get(entry.task);
  const project = task ? taskProject(view.workspace, task) : undefined;
  const color = taskColor(view.workspace, task);
  const running = entry.end === null;
  const end = entry.end ?? now;
  const [picker, setPicker] = useState<DOMRect | null>(null);
  const taskButton = useRef<HTMLButtonElement>(null);
  const moveField = useRef<HTMLButtonElement>(null);
  const [split, setSplit] = useState(() => Math.round((entry.start + end) / 2 / 60_000) * 60_000);
  const [note, setNote] = useState(entry.note ?? "");
  useEffect(() => setNote(entry.note ?? ""), [entry.note]);

  const splitValid = split > entry.start && split < end;
  const previousEnd = [...view.entries.values()]
    .filter((e) => e.id !== entry.id && e.end !== null && e.end <= entry.start)
    .reduce<Millis | undefined>((latest, e) => (latest === undefined || e.end! > latest ? e.end! : latest), undefined);
  const overlaps = [...view.entries.values()].filter((e) => e.id !== entry.id && e.start < end && (e.end ?? now) > entry.start);

  const safe = (fn: () => void) => {
    try {
      fn();
    } catch (error) {
      if (!(error instanceof DomainError)) throw error;
    }
  };
  const shiftStart = (delta: number) => safe(() => void actions.updateEntry(entry.id, { start: entry.start + delta }, `Moved start to ${formatClock(tz, entry.start + delta)}`));
  const shiftEnd = (delta: number) =>
    !running && safe(() => void actions.updateEntry(entry.id, { end: entry.end! + delta }, `Moved end to ${formatClock(tz, entry.end! + delta)}`));
  const grid = Math.max(snapMinutes, 1) * 60_000;
  const round = () =>
    safe(() =>
      void actions.updateEntry(
        entry.id,
        { start: Math.floor(entry.start / grid) * grid, ...(running ? {} : { end: Math.ceil(entry.end! / grid) * grid }) },
        `Rounded to ${snapMinutes || 1}m`,
      ),
    );

  useKeys({
    escape: () => onClose(),
    s: () => (running ? actions.stop(entry.id) : void actions.start(entry.task)),
    "shift+enter": () => void (splitValid && actions.split(entry.id, split)),
    "mod+m": () => setPicker(moveField.current?.getBoundingClientRect() ?? null),
    "backspace|delete": () => {
      actions.deleteEntry(entry.id);
      onClose();
    },
    "[": () => shiftStart(-MIN15),
    "]": () => shiftStart(MIN15),
    "{": () => shiftEnd(-MIN15),
    "}": () => shiftEnd(MIN15),
  });

  return (
    <>
      <div className="flex items-start gap-[10px] border-b px-4 pb-3 pt-[14px]">
        <span className="mt-[5px] size-[10px] shrink-0 rounded-xs" style={{ background: color }} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-[11px] text-faint">
            {task && <span className="font-mono">#{task.seq}</span>}
            {project && <span>{project.name}</span>}
            {running && (
              <span className="flex items-center gap-1 text-green-fg">
                <span className="size-[6px] rounded-full bg-green" />
                running {formatElapsed(entryDuration(entry, now))}
              </span>
            )}
          </div>
          <SheetTitle className="mt-[2px] text-[15px] font-medium leading-[1.25]">{task?.title ?? "(deleted task)"}</SheetTitle>
          <SheetDescription className="sr-only">Edit entry</SheetDescription>
        </div>
        <button type="button" onClick={onClose} className="mt-1 flex items-center gap-[6px] text-[12px] text-faint hover:text-fg" aria-label="Close">
          <Kbd>Esc</Kbd>
          <X size={12} />
        </button>
      </div>

      <div className="tt-scroll flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-[14px]">
        <Field label="Task">
          <button
            ref={taskButton}
            type="button"
            onClick={() => setPicker(taskButton.current?.getBoundingClientRect() ?? null)}
            className="tt-input min-h-9 gap-2 bg-base text-left text-[13px]"
          >
            <span className="size-2 shrink-0 rounded-xs" style={{ background: color }} />
            {task && <Seq seq={task.seq} className="text-[11px]" />}
            <span className="min-w-0 flex-1 truncate">{task?.title ?? "(deleted task)"}</span>
            {task && <TagChips view={view} task={task} size="xs" max={2} />}
            <CaretDown size={12} className="text-faint" />
          </button>
        </Field>

        <div className="grid grid-cols-2 gap-[10px]">
          <TimeField
            label="Start"
            value={entry.start}
            tz={tz}
            onCommit={(at) => actions.updateEntry(entry.id, { start: at }, `Changed start to ${formatClock(tz, at)}`)}
            day={formatDayShort(localDate(tz, entry.start))}
            dayOf={entry.start}
          />
          <TimeField
            label="End"
            value={entry.end}
            tz={tz}
            onCommit={(at) => actions.updateEntry(entry.id, { end: at }, running ? `Stopped at ${formatClock(tz, at!)}` : `Changed end to ${formatClock(tz, at!)}`)}
            day={running ? `${formatClock(tz, now)} · set end to stop` : formatDayShort(localDate(tz, entry.end!))}
            dayOf={entry.start}
            runningLabel="now"
          />
        </div>
        <div className="-mt-[6px] flex flex-wrap gap-[6px] text-[11.5px] text-muted">
          <Chip onClick={() => shiftStart(-MIN15)} title="Start −15m  [">−15m</Chip>
          <Chip onClick={() => shiftStart(MIN15)} title="Start +15m  ]">+15m</Chip>
          {previousEnd !== undefined && previousEnd !== entry.start && (
            <Chip onClick={() => safe(() => void actions.updateEntry(entry.id, { start: previousEnd }, `Start at previous end ${formatClock(tz, previousEnd)}`))}>
              Start at previous end {formatClock(tz, previousEnd)}
            </Chip>
          )}
          <Chip onClick={round}>Round to {snapMinutes || 1}m</Chip>
        </div>

        <Field label="Note">
          <textarea
            value={note}
            onChange={(e) => setNote(e.target.value)}
            onBlur={() => note !== (entry.note ?? "") && safe(() => void actions.updateEntry(entry.id, { note: note || null }, "Edited note"))}
            rows={3}
            className="tt-input min-h-[70px] resize-y bg-base px-[10px] py-[7px] text-[13px] leading-[1.45]"
            placeholder="What was this about?"
          />
        </Field>

        <div className="flex flex-col gap-[6px]">
          <div className="flex justify-between text-[11px] text-muted">
            <span>Split at</span>
            <span className="text-faint">drag the marker or type a time</span>
          </div>
          <SplitBar entry={entry} end={end} color={color} at={split} onChange={setSplit} tz={tz} running={running} />
          <div className="flex items-center gap-2 text-[12px]">
            <SplitInput value={split} tz={tz} dayOf={entry.start} onChange={setSplit} />
            <button
              type="button"
              disabled={!splitValid}
              onClick={() => actions.split(entry.id, split)}
              className="flex items-center gap-[6px] text-link disabled:text-faint"
            >
              Split here <Kbd>⇧⏎</Kbd>
            </button>
            <span className="truncate text-faint">
              → {formatClock(tz, entry.start)}–{formatClock(tz, split)} and {formatClock(tz, split)}–{running ? "now" : formatClock(tz, end)}
            </span>
          </div>
        </div>

        <div className="flex flex-col gap-[6px]">
          <div className="text-[11px] text-muted">Move to task</div>
          <button
            ref={moveField}
            type="button"
            onClick={() => setPicker(moveField.current?.getBoundingClientRect() ?? null)}
            className="flex h-[34px] items-center gap-2 rounded-md border border-s1 bg-base px-[10px] text-[12.5px] text-faint hover:border-s2"
          >
            <span>Type to search tasks, tags or ticket ids…</span>
            <Kbd className="ml-auto">{MOD}M</Kbd>
          </button>
        </div>

        <div className="mt-auto flex flex-col gap-[3px] text-[11.5px] text-faint">
          <span>
            Created {formatClock(tz, entry.created)} {formatDayShort(localDate(tz, entry.created))}
            {entry.updated !== entry.created && ` · edited ${formatClock(tz, entry.updated)}`}
          </span>
          {overlaps.map((o) => {
            const t = view.workspace.tasks.get(o.task);
            return (
              <span key={o.id}>
                Overlaps{" "}
                <span className="text-sub1">
                  {t ? `#${t.seq} ` : ""}
                  {t?.title ?? "(deleted task)"}
                </span>{" "}
                {formatClock(tz, Math.max(o.start, entry.start))} → {o.end === null && running ? "now" : formatClock(tz, Math.min(o.end ?? now, end))}
              </span>
            );
          })}
        </div>
      </div>

      <div className="flex items-center gap-2 border-t px-4 py-3">
        {running ? (
          <Button variant="primary" onClick={() => actions.stop(entry.id)} className="gap-2">
            <Stop size={11} weight="fill" /> Stop <Kbd inherit>S</Kbd>
          </Button>
        ) : (
          <Button variant="primary" onClick={() => actions.start(entry.task)} className="gap-2">
            <Play size={11} weight="fill" /> Start again <Kbd inherit>S</Kbd>
          </Button>
        )}
        <Button variant="secondary" onClick={() => actions.duplicateEntry(entry.id)}>
          Duplicate
        </Button>
        <Button
          variant="destructive"
          className="ml-auto gap-2"
          onClick={() => {
            actions.deleteEntry(entry.id);
            onClose();
          }}
        >
          Delete <Kbd inherit>⌫</Kbd>
        </Button>
      </div>
      <div className="px-4 pb-3 text-[11px] text-faint">Delete is immediate and undoable — no confirmation dialog.</div>

      <TaskPicker
        open={!!picker}
        anchor={picker}
        side="bottom"
        exclude={entry.task}
        onClose={() => setPicker(null)}
        onPick={(t) => {
          actions.relink(entry.id, t.id);
          setPicker(null);
        }}
        onCreate={(input) => {
          const t = actions.createTask(input, { quiet: true });
          if (t) actions.relink(entry.id, t.id);
          setPicker(null);
        }}
      />
    </>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="block">
      <div className="mb-[5px] text-[12px] text-sub1">{label}</div>
      {children}
    </div>
  );
}

function Chip({ children, onClick, title }: { children: ReactNode; onClick: () => void; title?: string }) {
  return (
    <button type="button" title={title} onClick={onClick} className="rounded-2 border border-s1 px-2 py-[3px] hover:border-s2 hover:text-fg">
      {children}
    </button>
  );
}

/** A clock on the entry's own day, or anything `parseTime` accepts (`-15m`, `yesterday 9:00`, ISO). */
function parseOnDay(text: string, tz: string, dayOf: Millis, now: Millis): Millis {
  const clock = parseClock(text.trim());
  if (clock) return localToUtc(tz, { ...localDate(tz, dayOf), ...clock });
  return parseTime(text, now, tz);
}

function TimeField({
  label,
  value,
  tz,
  onCommit,
  day,
  dayOf,
  runningLabel,
}: {
  label: string;
  value: Millis | null;
  tz: string;
  onCommit: (at: Millis) => void;
  day: string;
  dayOf: Millis;
  runningLabel?: string;
}) {
  const shown = value === null ? (runningLabel ?? "") : formatClock(tz, value);
  const [text, setText] = useState(shown);
  const [error, setError] = useState<string>();
  useEffect(() => {
    setText(shown);
    setError(undefined);
  }, [shown]);
  const commit = () => {
    if (text.trim() === shown) return setError(undefined);
    try {
      onCommit(parseOnDay(text, tz, dayOf, Date.now()));
      setError(undefined);
    } catch (e) {
      // Inline error; the entry is unchanged.
      setError(e instanceof Error ? e.message.replace(/ \d{4}-\d\d-\d\dT[\d:.]+Z/g, "") : String(e));
    }
  };
  const id = `field-${label.toLowerCase()}`;
  return (
    <div>
      <label htmlFor={id} className="mb-[5px] block text-[12px] text-sub1">
        {label}
      </label>
      <div className="tt-input min-h-9 gap-2 bg-base" aria-invalid={error ? true : undefined}>
        <input
          id={id}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            } else if (e.key === "Escape") {
              setText(shown);
              setError(undefined);
            }
          }}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${id}-error` : undefined}
          className={cn("w-[64px] min-w-0 bg-transparent text-[14px] tabular-nums outline-none", value === null && text === runningLabel && "text-green-fg")}
        />
        <span className="ml-auto truncate text-[11px] text-faint">{day}</span>
      </div>
      {error && (
        <div id={`${id}-error`} role="alert" className="mt-1 text-[11.5px] text-red-fg">
          {error}
        </div>
      )}
    </div>
  );
}

function SplitInput({ value, tz, dayOf, onChange }: { value: Millis; tz: string; dayOf: Millis; onChange: (at: Millis) => void }) {
  const [text, setText] = useState(formatClock(tz, value));
  useEffect(() => setText(formatClock(tz, value)), [value, tz]);
  return (
    <input
      aria-label="Split time"
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        try {
          onChange(parseOnDay(e.target.value, tz, dayOf, Date.now()));
        } catch {
          // wait for a complete time
        }
      }}
      className="w-[54px] rounded-2 border border-s1 bg-base px-[6px] py-[2px] text-[12px] tabular-nums outline-none focus:border-accent"
    />
  );
}

function SplitBar({ entry, end, color, at, onChange, tz, running }: { entry: Entry; end: Millis; color: string; at: Millis; onChange: (at: Millis) => void; tz: string; running: boolean }) {
  const bar = useRef<HTMLDivElement>(null);
  const span = Math.max(end - entry.start, 1);
  const pct = Math.min(100, Math.max(0, ((at - entry.start) / span) * 100));
  const fromPointer = (event: PointerEvent) => {
    const rect = bar.current!.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
    onChange(Math.round((entry.start + ratio * span) / 60_000) * 60_000);
  };
  return (
    <div
      ref={bar}
      role="slider"
      aria-label="Split marker"
      aria-valuemin={entry.start}
      aria-valuemax={end}
      aria-valuenow={at}
      aria-valuetext={formatClock(tz, at)}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft") onChange(at - 60_000 * (e.shiftKey ? 15 : 1));
        if (e.key === "ArrowRight") onChange(at + 60_000 * (e.shiftKey ? 15 : 1));
      }}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        fromPointer(e);
      }}
      onPointerMove={(e) => e.buttons && fromPointer(e)}
      className="relative h-[38px] cursor-ew-resize touch-none overflow-hidden rounded-2"
      style={{ background: `color-mix(in srgb, ${color} 18%, var(--c-base))`, boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${color} 25%, transparent)` }}
    >
      <div className="absolute inset-y-0 left-0 border-r-2 border-accent" style={{ width: `${pct}%` }} />
      <span className="absolute left-2 top-[11px] text-[11px] tabular-nums text-sub1">{formatClock(tz, entry.start)}</span>
      <span
        className="absolute -top-px rounded-b-sm bg-accent px-[6px] py-[2px] text-[10.5px] tabular-nums text-base"
        style={{ left: `calc(${pct}% - 20px)` }}
      >
        {formatClock(tz, at)}
      </span>
      <span className={cn("absolute right-2 top-[11px] text-[11px]", running ? "text-green-fg" : "tabular-nums text-sub1")}>{running ? "now" : formatClock(tz, end)}</span>
    </div>
  );
}

