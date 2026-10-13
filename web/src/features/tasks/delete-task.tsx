import { DotsThree } from "@phosphor-icons/react";
import { type ReactNode, useState } from "react";

import type { Task } from "@tt/domain";

import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Kbd } from "@/components/ui/kbd";
import { useActions } from "@/data/actions";
import { cn } from "@/lib/utils";

interface Pending {
  tasks: { task: Task; count: number }[];
  total: number;
  onDeleted?: () => void;
}

/**
 * Delete flow shared by the detail page, list rows, board cards and bulk
 * selections: tasks without entries go immediately (undo toast), any with
 * entries ask once first. Render `dialog` once next to the caller.
 */
export function useDeleteTask(): { request: (tasks: Task | Task[], onDeleted?: () => void) => void; dialog: ReactNode } {
  const actions = useActions();
  const [pending, setPending] = useState<Pending>();

  const run = async (tasks: Task[], force: boolean, onDeleted?: () => void) => {
    const ok = tasks.length === 1 ? await actions.deleteTask(tasks[0]!.id, { force }) : await actions.deleteTasks(tasks.map((t) => t.id), { force });
    if (ok) onDeleted?.();
  };
  const request = (input: Task | Task[], onDeleted?: () => void) => {
    const tasks = Array.isArray(input) ? input : [input];
    if (!tasks.length) return;
    const counted = tasks.map((task) => ({ task, count: actions.entriesOf(task.id).length }));
    const total = counted.reduce((sum, c) => sum + c.count, 0);
    if (total === 0) void run(tasks, false, onDeleted);
    else setPending({ tasks: counted, total, onDeleted });
  };

  const confirm = () => {
    if (!pending) return;
    const { tasks, onDeleted } = pending;
    setPending(undefined);
    // The count is re-read at confirm time: the action deletes the current set.
    void run(
      tasks.map((t) => t.task),
      true,
      onDeleted,
    );
  };

  const entriesWord = (n: number) => (n === 1 ? "entry" : "entries");
  const dialog = (
    <Dialog open={!!pending} onOpenChange={(open) => !open && setPending(undefined)}>
      <DialogContent className="top-[20vh] w-[min(440px,calc(100vw-24px))] p-4">
        {pending && pending.tasks.length === 1 && (
          <>
            <DialogTitle className="text-[15px] font-medium">
              Delete #{pending.tasks[0]!.task.seq} {pending.tasks[0]!.task.title}?
            </DialogTitle>
            <DialogDescription className="mt-2 text-[13px] text-muted">
              This task has {pending.total} time {entriesWord(pending.total)}. They will be deleted too.
            </DialogDescription>
            <DialogActions label={`Delete task and ${pending.total} ${entriesWord(pending.total)}`} onConfirm={confirm} />
          </>
        )}
        {pending && pending.tasks.length > 1 && (
          <>
            <DialogTitle className="text-[15px] font-medium">
              Delete {pending.tasks.length} tasks and {pending.total} {entriesWord(pending.total)}?
            </DialogTitle>
            <DialogDescription className="mt-2 text-[13px] text-muted">
              These tasks have time entries. They will be deleted too.
            </DialogDescription>
            <ul className="mt-3 max-h-[156px] overflow-y-auto rounded-2 border text-[12px]" data-testid="bulk-delete-list">
              {pending.tasks
                .filter((t) => t.count > 0)
                .map(({ task, count }) => (
                  <li key={task.id} className="flex h-[26px] items-center gap-2 px-2">
                    <span className="font-mono text-faint">#{task.seq}</span>
                    <span className="min-w-0 flex-1 truncate">{task.title}</span>
                    <span className="tabular-nums text-muted">
                      {count} {entriesWord(count)}
                    </span>
                  </li>
                ))}
            </ul>
            <DialogActions label={`Delete ${pending.tasks.length} tasks and ${pending.total} ${entriesWord(pending.total)}`} onConfirm={confirm} />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
  return { request, dialog };
}

function DialogActions({ label, onConfirm }: { label: string; onConfirm: () => void }) {
  return (
    <div className="mt-4 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
      <DialogClose asChild>
        <Button variant="secondary" autoFocus className="h-11 sm:h-auto">
          Cancel
        </Button>
      </DialogClose>
      <Button
        variant="destructive"
        className="h-11 border-[color-mix(in_srgb,var(--ctp-red)_45%,transparent)] sm:h-auto"
        onClick={onConfirm}
      >
        {label}
      </Button>
    </div>
  );
}

/** `⋯` overflow menu of a task; Delete is always last, after a separator. */
export function TaskMenu({
  task,
  onDelete,
  open,
  onOpenChange,
  className,
  stopPointer,
}: {
  task: Task;
  onDelete: () => void;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Positioning and reveal classes for the trigger. */
  className?: string;
  /** Keep presses and keys from reaching the drag listeners of an ancestor. */
  stopPointer?: boolean;
}) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={`Actions for #${task.seq} ${task.title}`}
          data-testid="task-menu"
          onClick={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
          onPointerDown={stopPointer ? (e) => e.stopPropagation() : undefined}
          onKeyDown={stopPointer ? (e) => e.stopPropagation() : undefined}
          className={cn(
            "flex flex-none items-center justify-center rounded-2 text-muted outline-none hover:bg-[color-mix(in_srgb,var(--c-text)_7%,transparent)] hover:text-fg focus-visible:outline-2 focus-visible:outline-accent data-[state=open]:bg-[color-mix(in_srgb,var(--c-text)_7%,transparent)] data-[state=open]:text-fg",
            className,
          )}
        >
          <DotsThree size={16} weight="bold" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={4} className="w-[212px]" onClick={(e) => e.stopPropagation()}>
        <DropdownMenuLabel className="truncate">
          #{task.seq} {task.title}
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={onDelete}
          data-testid="task-menu-delete"
          className="h-7 text-red-fg data-[highlighted]:bg-[color-mix(in_srgb,var(--ctp-red)_14%,transparent)] data-[highlighted]:shadow-none pointer-coarse:h-11"
        >
          Delete
          <Kbd className="ml-auto text-red-fg pointer-coarse:hidden">⌫</Kbd>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
