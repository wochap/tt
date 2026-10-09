// Renumber notice (Turn 5, frames 5.13/5.15): when a seq collision moves a
// task to a new number, one toast says so; a burst collapses into a count
// (toast id `renumber`) that lists each change when expanded.

import { CaretUp } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";

import { useStore } from "@/data/react";
import type { Renumbering } from "@/data/store";
import { cn } from "@/lib/utils";

const TOAST_ID = "renumber";
const DURATION = 10_000;

/** One line for a single change, a count with an expandable list for several. */
export function RenumberToast({ changes, onView }: { changes: Renumbering[]; onView: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  if (changes.length === 1) {
    const [change] = changes as [Renumbering];
    return (
      <span>
        Task <span className="font-mono text-muted">#{change.from}</span> is now <span className="font-mono">#{change.to}</span>{" "}
        <span className="text-muted">· {change.title}</span>
      </span>
    );
  }
  return (
    <div className="flex w-[360px] flex-col" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <button type="button" aria-expanded={open} className="flex h-8 items-center gap-3 text-left" onClick={() => setOpen(!open)}>
        <span>{changes.length} tasks renumbered</span>
        <CaretUp size={11} className={cn("ml-auto text-faint transition-transform", !open && "rotate-180")} />
      </button>
      {open && (
        <ul aria-label="Renumbered tasks">
          {changes.map((change) => (
            <li key={change.id} className="flex h-[30px] items-center gap-[10px] border-t border-s0">
              <span className="w-[30px] font-mono text-muted">#{change.from}</span>
              <span className="text-faint">→</span>
              <span className="w-[30px] font-mono">#{change.to}</span>
              <span className="min-w-0 flex-1 truncate text-sub1">{change.title}</span>
              <button type="button" className="rounded-sm px-2 py-1 text-link" onClick={() => onView(change.id)}>
                View
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Shows the renumber toast for store renumberings; mount once in the shell. */
export function RenumberNotices() {
  const store = useStore();
  const navigate = useNavigate();
  // Changes in the visible toast; a new burst adds to them.
  const shown = useRef<Renumbering[]>([]);
  useEffect(() => {
    const view = (id: string) => {
      const seq = store.view.workspace.tasks.get(id)?.seq;
      if (seq !== undefined) void navigate(`/tasks/${seq}`);
      toast.dismiss(TOAST_ID);
    };
    const reset = () => {
      shown.current = [];
    };
    return store.onRenumber((changes) => {
      const merged = new Map(shown.current.map((c) => [c.id, c]));
      for (const change of changes) {
        const earlier = merged.get(change.id);
        merged.set(change.id, earlier ? { ...change, from: earlier.from } : change);
      }
      shown.current = [...merged.values()];
      const list = shown.current;
      toast(<RenumberToast changes={list} onView={view} />, {
        id: TOAST_ID,
        duration: DURATION,
        action: list.length === 1 ? { label: "View", onClick: () => view(list[0]!.id) } : undefined,
        onDismiss: reset,
        onAutoClose: reset,
      });
    });
  }, [store, navigate]);
  return null;
}
