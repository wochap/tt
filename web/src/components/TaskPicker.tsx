// Command-based task picker (frame 15): anchored to a rect (the drag ghost,
// a field), one fuzzy index over title, tags, metadata values; recently
// tracked first; "Create task" parses the quick-create syntax.

import { Popover as P } from "radix-ui";
import { type ReactNode, useMemo, useRef, useState } from "react";

import { type NewTask, parseQuick, search, type Task, taskTags, type Uuid } from "@tt/domain";

import { Highlight, Seq } from "@/components/task-bits";
import { Command, CommandInput, CommandItem, CommandList, CommandSeparator } from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import { useView } from "@/data/react";
import { pickerTasks, runningTaskIds } from "@/data/select";
import { taskColor } from "@/lib/colors";
import { MOD } from "@/lib/utils";

export interface PickOptions {
  /** ⇧⏎: link and keep the new entry running. */
  keepRunning: boolean;
}

export interface TaskPickerProps {
  open: boolean;
  /** Viewport rect to anchor to (right edge; flips left near the edge). */
  anchor: DOMRect | null;
  side?: "right" | "bottom";
  onPick: (task: Task, options: PickOptions) => void;
  onCreate: (input: NewTask, options: PickOptions) => void;
  onClose: () => void;
  headerHint?: ReactNode;
  /** Show the "keep running" binding (only meaningful when the range ends now). */
  canKeepRunning?: boolean;
  exclude?: Uuid;
  placeholder?: string;
}

export function TaskPicker({ open, anchor, side = "right", onClose, ...rest }: TaskPickerProps) {
  const virtual = useRef({ getBoundingClientRect: () => anchor ?? new DOMRect() });
  virtual.current.getBoundingClientRect = () => anchor ?? new DOMRect();
  return (
    <P.Root open={open && anchor !== null} onOpenChange={(next) => !next && onClose()}>
      <P.Anchor virtualRef={virtual} />
      <P.Portal>
        <P.Content
          side={side}
          align="start"
          sideOffset={12}
          collisionPadding={12}
          onOpenAutoFocus={(event) => event.preventDefault()}
          className="z-50 w-[300px] overflow-hidden rounded-md bg-mantle text-fg shadow-[var(--shadow-lg)] outline-none"
          data-testid="task-picker"
        >
          {open && <PickerBody onClose={onClose} {...rest} />}
        </P.Content>
      </P.Portal>
    </P.Root>
  );
}

function PickerBody({ onPick, onCreate, onClose, headerHint, canKeepRunning = false, exclude, placeholder }: Omit<TaskPickerProps, "open" | "anchor" | "side">) {
  const view = useView();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState("");
  const running = runningTaskIds(view);
  const hits = useMemo(() => {
    const q = query.trim();
    const filter = (t: Task) => t.state !== "archived" && t.id !== exclude;
    if (!q) return pickerTasks(view).filter(filter).slice(0, 8).map((task) => ({ task, matches: [] as ReturnType<typeof search>[number]["matches"] }));
    return search(view, q, filter).slice(0, 8);
  }, [query, view, exclude]);

  const pick = (task: Task, keepRunning: boolean) => onPick(task, { keepRunning: keepRunning && canKeepRunning });
  const create = (keepRunning: boolean) => {
    const input = parseQuick(query);
    if (input.title.trim()) onCreate(input, { keepRunning: keepRunning && canKeepRunning });
  };

  return (
    <Command
      value={selected}
      onValueChange={setSelected}
      label="Pick a task"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          create(event.shiftKey);
        } else if (event.key === "Enter" && event.shiftKey) {
          event.preventDefault();
          const hit = hits.find((h) => h.task.id === selected) ?? hits[0];
          if (hit) pick(hit.task, true);
        }
      }}
    >
      <div className="flex h-[38px] items-center gap-2 border-b px-3 text-[13px]">
        <CommandInput autoFocus value={query} onValueChange={setQuery} placeholder={placeholder ?? "Search tasks, tags, ticket ids…"} />
        {headerHint && <span className="ml-auto shrink-0 text-[11px] text-muted">{headerHint}</span>}
      </div>
      <CommandList className="max-h-[300px] text-[12.5px]">
        {hits.map(({ task, matches }) => {
          const title = matches.find((m) => m.kind === "title");
          const meta = matches.find((m) => m.kind === "meta");
          const project = task.project ? view.workspace.projects.get(task.project) : undefined;
          const tag = taskTags(view.workspace, task)[0];
          return (
            <CommandItem key={task.id} value={task.id} onSelect={() => pick(task, false)}>
              <span className="size-2 shrink-0 rounded-xs" style={{ background: taskColor(view.workspace, task) }} />
              <Seq seq={task.seq} />
              <span className="min-w-0 flex-1 truncate">
                <Highlight text={task.title} indices={title?.indices} />
              </span>
              <span className="shrink-0 text-[11px] text-muted">
                {running.has(task.id) ? (
                  <span className="text-green-fg">running</span>
                ) : meta ? (
                  <span className="font-mono">
                    <Highlight text={meta.text} indices={meta.indices} mark="yellow" />
                  </span>
                ) : (
                  (project?.name ?? (tag ? `+${tag.name}` : ""))
                )}
              </span>
            </CommandItem>
          );
        })}
        {query.trim() && (
          <>
            {hits.length > 0 && <CommandSeparator />}
            <CommandItem value="__create" onSelect={() => create(false)} className="text-muted">
              <span className="w-2 text-center text-link">+</span>
              <span className="flex-1 truncate">Create task “{parseQuick(query).title || query}”</span>
              <Kbd>{MOD}⏎</Kbd>
            </CommandItem>
          </>
        )}
        {!query.trim() && hits.length === 0 && <div className="px-3 py-3 text-[12px] text-muted">No tasks yet — type a title to create one.</div>}
      </CommandList>
      <div className="flex gap-3 border-t px-3 py-[7px] text-[11px] text-muted">
        <span>
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
        </span>
        <span>
          <Kbd>⏎</Kbd> link
        </span>
        {canKeepRunning && (
          <span>
            <Kbd>⇧⏎</Kbd> link &amp; keep running
          </span>
        )}
        <span className="ml-auto">
          <Kbd>Esc</Kbd> discard
        </span>
      </div>
    </Command>
  );
}
