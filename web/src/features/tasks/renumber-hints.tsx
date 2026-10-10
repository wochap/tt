import type { RenumberedFrom, Task } from "@tt/domain";

import { cn } from "@/lib/utils";

/**
 * Tasks renumbered away from the short id the query names, ordered by their
 * current seq. Only an exact short-id query (`#12`) has hints.
 */
export function renumberHints(tasks: Iterable<Task>, query: string): RenumberedFrom[] {
  const match = /^#(\d+)$/.exec(query.trim());
  if (!match) return [];
  const seq = Number(match[1]);
  return [...tasks]
    .filter((task) => task.seq !== seq && (task.previousSeqs ?? []).includes(seq))
    .map((task) => ({ id: task.id, title: task.title, from: seq, to: task.seq }))
    .sort((a, b) => a.to - b.to);
}

/** Muted, non-selectable row under the task that holds `#from` now: "#to title was renumbered from #from". */
export function RenumberHint({ hint, onOpen, className }: { hint: RenumberedFrom; onOpen: () => void; className?: string }) {
  return (
    <div className={cn("flex min-h-8 items-center gap-2 text-[12px] text-muted", className)} data-testid="renumber-hint" aria-disabled="true">
      <span className="text-faint">↳</span>
      <span className="min-w-0 flex-1 text-pretty">
        <span className="font-mono text-sub1">#{hint.to}</span> <span className="text-sub1">{hint.title}</span> was renumbered from{" "}
        <span className="font-mono">#{hint.from}</span>
      </span>
      <button
        type="button"
        tabIndex={-1}
        className="flex-none text-link hover:underline"
        onPointerDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.stopPropagation();
          onOpen();
        }}
      >
        Open #{hint.to}
      </button>
    </div>
  );
}
