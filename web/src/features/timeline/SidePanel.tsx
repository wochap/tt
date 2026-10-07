import { useDroppable } from "@dnd-kit/core";
import { Play, Stop } from "@phosphor-icons/react";
import { useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";

import { type Entry, formatDuration, type Range, search, type Task, type Uuid } from "@tt/domain";

import { Seq } from "@/components/task-bits";
import { ColorDot } from "@/components/ui/badge";
import { Kbd } from "@/components/ui/kbd";
import { useActions } from "@/data/actions";
import { useView } from "@/data/react";
import { pickerTasks, runningTaskIds, taskTotals } from "@/data/select";
import { taskColor } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { cn } from "@/lib/utils";

import { useUi } from "../shell/ui-state.tsx";
import { TotalsGrid } from "./TotalsBar.tsx";

/** Task list beside the grid: drop an entry on a row to re-link it; totals below. */
export function SidePanel({
  width,
  range,
  entries,
  now,
  title,
  showToday,
  overTask,
  dragging,
}: {
  width: number;
  range: Range;
  entries: Entry[];
  now: number;
  title: string;
  showToday: boolean;
  overTask?: Uuid;
  dragging: boolean;
}) {
  const view = useView();
  const [filter, setFilter] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const { openPalette } = useUi();
  useKeys({ "/": () => input.current?.focus() });

  const tasks = useMemo(() => {
    if (filter.trim()) return search(view, filter, (t) => t.state !== "archived").map((h) => h.task).slice(0, 40);
    const all = pickerTasks(view);
    const open = all.filter((t) => t.state === "open");
    const done = all.filter((t) => t.state === "done").slice(0, 3);
    return [...open.slice(0, 40), ...done];
  }, [view, filter]);
  const today = useMemo(() => taskTotals(view, now, range), [view, now, range]);
  const running = runningTaskIds(view);
  const runningCount = entries.filter((e) => e.end === null).length;

  return (
    <aside className="flex min-h-0 flex-none flex-col border-l bg-mantle" style={{ width }} aria-label="Tasks">
      <div className="flex flex-col gap-2 px-3 pb-2 pt-[10px]">
        <div className="tt-label flex items-center justify-between">
          <span>Tasks</span>
          <span className="normal-case tracking-normal text-faint">{width > 250 ? "drop an entry to re-link" : "drop to re-link"}</span>
        </div>
        <label className="flex h-7 items-center justify-between rounded-2 border border-s1 bg-base px-[9px] text-[12px] focus-within:border-accent">
          <input
            ref={input}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && (setFilter(""), input.current?.blur())}
            placeholder="Filter tasks…"
            aria-label="Filter tasks"
            className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-faint"
          />
          <Kbd>/</Kbd>
        </label>
      </div>
      <div className="tt-scroll flex min-h-0 flex-1 flex-col overflow-y-auto">
        {tasks.map((task) => (
          <TaskRow
            key={task.id}
            task={task}
            today={showToday ? today.get(task.id) : undefined}
            running={running.has(task.id)}
            target={overTask === task.id}
            dragging={dragging}
          />
        ))}
        <button
          type="button"
          onClick={() => openPalette("create", filter)}
          className="mx-3 my-2 flex items-center gap-[6px] rounded-2 border border-dashed border-s1 px-[10px] py-2 text-left text-[11.5px] text-faint hover:border-s2 hover:text-fg"
        >
          <span className="text-link">+</span>New task <Kbd>C</Kbd>
        </button>
      </div>
      <div className="border-t px-3 py-[10px]">
        <TotalsGrid entries={entries} now={now} range={range} running={runningCount} title={title} />
      </div>
    </aside>
  );
}

function TaskRow({ task, today, running, target, dragging }: { task: Task; today?: number; running: boolean; target: boolean; dragging: boolean }) {
  const view = useView();
  const actions = useActions();
  const navigate = useNavigate();
  const drop = useDroppable({ id: `task:${task.id}`, data: { task: task.id } });
  const done = task.state === "done";
  return (
    <div
      ref={drop.setNodeRef}
      data-testid="side-task"
      data-seq={task.seq}
      className={cn(
        "group flex h-[34px] flex-none items-center gap-2 pl-3 pr-2 text-[12.5px]",
        target
          ? "mx-[6px] rounded-2 bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)] pl-[6px] shadow-[inset_0_0_0_1px_var(--color-accent)]"
          : !dragging && "tt-row-hover",
        done && "line-through opacity-50",
      )}
    >
      <ColorDot color={taskColor(view.workspace, task)} />
      <Seq seq={task.seq} className="w-[26px]" />
      <button type="button" onClick={() => navigate(`/tasks/${task.seq}`)} className="min-w-0 flex-1 truncate text-left hover:underline">
        {task.title}
      </button>
      {target ? (
        <span className="shrink-0 text-[11px] text-link">drop target</span>
      ) : (
        today !== undefined && today > 0 && <span className="shrink-0 text-[11px] tabular-nums text-muted">{formatDuration(today)}</span>
      )}
      <button
        type="button"
        aria-label={running ? `Stop ${task.title}` : `Start ${task.title}`}
        onClick={() => actions.toggle(task.id)}
        className={cn(
          "flex size-[22px] shrink-0 items-center justify-center rounded-sm border",
          running
            ? "border-[color-mix(in_srgb,var(--ctp-green)_45%,transparent)] text-green-fg"
            : "border-transparent text-transparent group-hover:border-s1 group-hover:text-link focus-visible:text-link",
        )}
      >
        {running ? <Stop size={9} weight="fill" /> : <Play size={9} weight="fill" />}
      </button>
    </div>
  );
}
