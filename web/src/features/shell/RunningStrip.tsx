import { Stop } from "@phosphor-icons/react";

import { type Entry, entryDuration, formatClock, formatElapsed } from "@tt/domain";

import { Seq } from "@/components/task-bits";
import { ColorDot } from "@/components/ui/badge";
import { Kbd } from "@/components/ui/kbd";
import { Tooltip } from "@/components/ui/tooltip";
import { useActions } from "@/data/actions";
import { useNow, useView } from "@/data/react";
import { taskColor, taskTextColor } from "@/lib/colors";
import { useTz } from "@/lib/settings";
import { cn } from "@/lib/utils";

/** Every running entry with a ticking counter, stop, and Stop all (⇧S). */
export function RunningStrip({ phone = false }: { phone?: boolean }) {
  const view = useView();
  const actions = useActions();
  const tz = useTz();
  const now = useNow(1000);
  const running = [...view.entries.values()]
    .filter((e) => e.end === null)
    .sort((a, b) => a.start - b.start || (a.id < b.id ? -1 : 1));

  if (phone) {
    if (!running.length) return null;
    return (
      <div className="flex flex-none flex-col gap-[6px] border-b bg-[color-mix(in_srgb,var(--ctp-green)_7%,var(--c-base))] px-[14px] pb-2 pt-[6px]" aria-label="Running">
        {running.map((entry) => (
          <PhoneRunning key={entry.id} entry={entry} now={now} />
        ))}
      </div>
    );
  }

  return (
    <div
      aria-label="Running"
      className={cn(
        "flex min-h-[41px] flex-none items-center gap-[10px] overflow-x-auto border-b px-[14px] py-[6px]",
        running.length ? "bg-[color-mix(in_srgb,var(--ctp-green)_7%,var(--c-base))]" : "bg-base",
      )}
    >
      <span className="tt-label flex shrink-0 items-center gap-[6px]">
        <span
          className={cn("size-[7px] rounded-full", running.length ? "bg-green animate-pulse-dot" : "bg-ov0")}
          style={running.length ? { boxShadow: "0 0 0 3px color-mix(in srgb, var(--ctp-green) 30%, transparent)" } : undefined}
        />
        {running.length ? `Running · ${running.length}` : "Nothing running"}
      </span>
      {running.map((entry) => {
        const task = view.workspace.tasks.get(entry.task);
        return (
          <div
            key={entry.id}
            data-testid="running-chip"
            className="flex h-7 shrink-0 items-center gap-2 rounded-2 border border-s1 bg-base pl-[10px] pr-1"
          >
            <ColorDot color={taskColor(view.workspace, task)} />
            {task && <Seq seq={task.seq} className="text-[11px]" />}
            <span className="max-w-[280px] truncate text-[12.5px] font-medium">{task?.title ?? "(deleted task)"}</span>
            <span className="text-[11px] text-muted">since {formatClock(tz, entry.start)}</span>
            <span className="ml-1 font-mono text-[12.5px] tabular-nums" style={{ color: taskTextColor(view.workspace, task) }}>
              {formatElapsed(entryDuration(entry, now))}
            </span>
            <Tooltip content="Stop">
              <button
                type="button"
                aria-label={`Stop ${task?.title ?? "entry"}`}
                onClick={() => actions.stop(entry.id)}
                className="flex size-[22px] items-center justify-center rounded-sm border border-[color-mix(in_srgb,var(--ctp-red)_35%,transparent)] text-red-fg hover:bg-[color-mix(in_srgb,var(--ctp-red)_10%,transparent)]"
              >
                <Stop size={10} weight="fill" />
              </button>
            </Tooltip>
          </div>
        );
      })}
      {running.length > 0 ? (
        <button
          type="button"
          onClick={() => actions.stopAll()}
          className="ml-auto flex shrink-0 items-center gap-[6px] rounded-2 px-[6px] py-[3px] text-[12px] text-muted hover:text-fg"
        >
          Stop all <Kbd>⇧S</Kbd>
        </button>
      ) : (
        <span className="ml-auto shrink-0 text-[12px] text-muted">
          <Kbd>S</Kbd> start the focused task · <Kbd>N</Kbd> new entry at now
        </span>
      )}
    </div>
  );
}

function PhoneRunning({ entry, now }: { entry: Entry; now: number }) {
  const view = useView();
  const actions = useActions();
  const task = view.workspace.tasks.get(entry.task);
  return (
    <div data-testid="running-chip" className="flex h-10 items-center gap-2 rounded-md border border-s1 bg-base pl-[10px] pr-[6px]">
      <ColorDot color={taskColor(view.workspace, task)} />
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{task?.title ?? "(deleted task)"}</span>
      <span className="font-mono text-[12.5px]" style={{ color: taskTextColor(view.workspace, task) }}>
        {formatElapsed(entryDuration(entry, now))}
      </span>
      <button
        type="button"
        aria-label={`Stop ${task?.title ?? "entry"}`}
        onClick={() => actions.stop(entry.id)}
        className="flex size-[30px] items-center justify-center rounded-2 border border-[color-mix(in_srgb,var(--ctp-red)_35%,transparent)] text-red-fg"
      >
        <Stop size={11} weight="fill" />
      </button>
    </div>
  );
}
