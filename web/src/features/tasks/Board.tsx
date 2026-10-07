import {
  DndContext,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  PointerSensor,
  KeyboardSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { useState } from "react";
import { useNavigate } from "react-router";

import { formatDuration, type Hit, type Task, type TaskState, TASK_STATES, type Uuid } from "@tt/domain";

import { Seq, TagChips } from "@/components/task-bits";
import { ColorDot } from "@/components/ui/badge";
import { Kbd } from "@/components/ui/kbd";
import { useActions } from "@/data/actions";
import { useView } from "@/data/react";
import { taskColor } from "@/lib/colors";
import { cn } from "@/lib/utils";

const TITLES: Record<TaskState, string> = { open: "Open", done: "Done", archived: "Archived" };
const DROP_TEXT: Record<TaskState, string> = { open: "Drop to reopen", done: "Drop to mark done", archived: "Drop to archive" };

/** Frame 21: tasks by state; drag cards between columns (undoable). */
export function Board({ hits, totals, focus, setFocus }: { hits: Hit[]; totals: Map<Uuid, number>; focus?: Uuid; setFocus: (id: Uuid) => void }) {
  const actions = useActions();
  const [dragging, setDragging] = useState<Task>();
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }), useSensor(KeyboardSensor));
  const byState = (state: TaskState) => hits.filter((h) => h.task.state === state).map((h) => h.task);

  const onStart = (event: DragStartEvent) => setDragging((event.active.data.current as { task: Task }).task);
  const onEnd = (event: DragEndEvent) => {
    setDragging(undefined);
    const task = (event.active.data.current as { task: Task }).task;
    const state = event.over?.data.current?.state as TaskState | undefined;
    if (state && state !== task.state) actions.setState(task.id, state);
  };

  return (
    <DndContext sensors={sensors} onDragStart={onStart} onDragEnd={onEnd} onDragCancel={() => setDragging(undefined)}>
      <div className="grid min-h-0 flex-1 grid-cols-3 gap-3 p-[14px]">
        {TASK_STATES.map((state) => (
          <Column key={state} state={state} tasks={byState(state)} totals={totals} focus={focus} setFocus={setFocus} dragging={dragging} />
        ))}
      </div>
      <div className="flex gap-[14px] border-t px-[14px] py-2 text-[11.5px] text-muted">
        <span>
          Drag cards between columns to change state (undoable). <Kbd>X</Kbd> on a focused card toggles done.
        </span>
        <span className="ml-auto">Archived hides from pickers but keeps its entries.</span>
      </div>
      <DragOverlay dropAnimation={null}>{dragging && <Card task={dragging} total={totals.get(dragging.id)} overlay />}</DragOverlay>
    </DndContext>
  );
}

function Column({ state, tasks, totals, focus, setFocus, dragging }: { state: TaskState; tasks: Task[]; totals: Map<Uuid, number>; focus?: Uuid; setFocus: (id: Uuid) => void; dragging?: Task }) {
  const drop = useDroppable({ id: `state:${state}`, data: { state } });
  const showDrop = dragging && dragging.state !== state;
  return (
    <section className="flex min-h-0 flex-col gap-2" aria-label={TITLES[state]} data-testid={`board-${state}`}>
      <div className="tt-label flex items-center justify-between px-[2px]">
        <span>{TITLES[state]}</span>
        <span className="font-mono">{tasks.length}</span>
      </div>
      <div
        ref={drop.setNodeRef}
        className={cn(
          "tt-scroll flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto rounded-md bg-mantle p-2",
          drop.isOver && showDrop && "shadow-[inset_0_0_0_1px_var(--color-accent)]",
        )}
      >
        {showDrop && (
          <div className="flex h-[62px] flex-none items-center justify-center rounded-md border border-dashed border-accent bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)] text-[11.5px] text-link">
            {DROP_TEXT[state]}
          </div>
        )}
        {tasks.map((task) => (
          <DraggableCard key={task.id} task={task} total={totals.get(task.id)} focused={focus === task.id} onFocus={() => setFocus(task.id)} hidden={dragging?.id === task.id} />
        ))}
      </div>
    </section>
  );
}

function DraggableCard({ task, total, focused, onFocus, hidden }: { task: Task; total?: number; focused: boolean; onFocus: () => void; hidden: boolean }) {
  const drag = useDraggable({ id: `card:${task.id}`, data: { task } });
  return (
    <div ref={drag.setNodeRef} {...drag.listeners} {...drag.attributes} onFocus={onFocus} className={cn("outline-none", hidden && "opacity-30")} data-testid="board-card" data-seq={task.seq}>
      <Card task={task} total={total} focused={focused} />
    </div>
  );
}

function Card({ task, total, focused, overlay }: { task: Task; total?: number; focused?: boolean; overlay?: boolean }) {
  const view = useView();
  const navigate = useNavigate();
  const project = task.project ? view.workspace.projects.get(task.project) : undefined;
  return (
    <div
      onDoubleClick={() => navigate(`/tasks/${task.seq}`)}
      className={cn(
        "flex cursor-grab flex-col gap-[6px] rounded-md bg-base px-[10px] py-2",
        overlay ? "rotate-[-2deg] cursor-grabbing shadow-[var(--shadow-lg)]" : "shadow-[var(--shadow-sm)]",
        focused && "outline-2 outline-offset-1 outline-accent",
      )}
    >
      <div className="flex min-w-0 items-center gap-2">
        <ColorDot color={taskColor(view.workspace, task)} />
        <Seq seq={task.seq} />
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium">{task.title}</span>
      </div>
      <div className="flex flex-wrap items-center gap-1">
        <TagChips view={view} task={task} />
        <span className="text-[11px] text-muted">{project?.name}</span>
        <span className="ml-auto text-[11px] tabular-nums text-muted">{total ? formatDuration(total) : "—"}</span>
      </div>
    </div>
  );
}
