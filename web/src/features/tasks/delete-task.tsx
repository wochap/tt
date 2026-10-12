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
  task: Task;
  count: number;
  onDeleted?: () => void;
}

/**
 * Delete flow shared by the detail page, list rows and board cards: a task
 * without entries goes immediately (undo toast), one with entries asks first.
 * Render `dialog` once next to the caller.
 */
export function useDeleteTask(): { request: (task: Task, onDeleted?: () => void) => void; dialog: ReactNode } {
  const actions = useActions();
  const [pending, setPending] = useState<Pending>();

  const run = async (task: Task, force: boolean, onDeleted?: () => void) => {
    if (await actions.deleteTask(task.id, { force })) onDeleted?.();
  };
  const request = (task: Task, onDeleted?: () => void) => {
    const count = actions.entriesOf(task.id).length;
    if (count === 0) void run(task, false, onDeleted);
    else setPending({ task, count, onDeleted });
  };

  const confirm = () => {
    if (!pending) return;
    const { task, onDeleted } = pending;
    setPending(undefined);
    // The count is re-read at confirm time: the action deletes the current set.
    void run(task, true, onDeleted);
  };

  const dialog = (
    <Dialog open={!!pending} onOpenChange={(open) => !open && setPending(undefined)}>
      <DialogContent className="top-[20vh] w-[min(440px,calc(100vw-24px))] p-4">
        {pending && (
          <>
            <DialogTitle className="text-[15px] font-medium">
              Delete #{pending.task.seq} {pending.task.title}?
            </DialogTitle>
            <DialogDescription className="mt-2 text-[13px] text-muted">
              This task has {pending.count} time {pending.count === 1 ? "entry" : "entries"}. They will be deleted too.
            </DialogDescription>
            <div className="mt-4 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
              <DialogClose asChild>
                <Button variant="secondary" autoFocus className="h-11 sm:h-auto">
                  Cancel
                </Button>
              </DialogClose>
              <Button
                variant="destructive"
                className="h-11 border-[color-mix(in_srgb,var(--ctp-red)_45%,transparent)] sm:h-auto"
                onClick={confirm}
              >
                Delete task and {pending.count} {pending.count === 1 ? "entry" : "entries"}
              </Button>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
  return { request, dialog };
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
