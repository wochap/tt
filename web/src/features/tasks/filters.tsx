import { CaretDown, X } from "@phosphor-icons/react";

import { type Hit, search, type Task, type TaskState, type Uuid, type View } from "@tt/domain";

import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { lastTracked } from "@/data/select";
import { cn } from "@/lib/utils";

export type StateFilter = TaskState | "any";
export type SortKey = "tracked" | "seq" | "title" | "total";

export interface TaskFilters {
  query: string;
  state: StateFilter;
  tags: Uuid[];
  projects: Uuid[];
  sort: SortKey;
}

export const DEFAULT_FILTERS: TaskFilters = { query: "", state: "open", tags: [], projects: [], sort: "tracked" };

/** Filtered, ranked tasks. A query ranks by match (exact ids first); otherwise by the sort key. */
export function filterTasks(view: View, filters: TaskFilters, totals: Map<Uuid, number>, ignoreState = false): Hit[] {
  const pass = (task: Task) =>
    (ignoreState || filters.state === "any" || task.state === filters.state) &&
    (!filters.tags.length || filters.tags.some((t) => task.tags.includes(t))) &&
    (!filters.projects.length || (task.project !== undefined && filters.projects.includes(task.project)));
  const hits = search(view, filters.query, pass);
  if (filters.query.trim()) return hits;
  const recent = lastTracked(view);
  const by: Record<SortKey, (a: Hit, b: Hit) => number> = {
    tracked: (a, b) => (recent.get(b.task.id) ?? -Infinity) - (recent.get(a.task.id) ?? -Infinity) || b.task.seq - a.task.seq,
    seq: (a, b) => b.task.seq - a.task.seq,
    title: (a, b) => a.task.title.localeCompare(b.task.title),
    total: (a, b) => (totals.get(b.task.id) ?? 0) - (totals.get(a.task.id) ?? 0) || b.task.seq - a.task.seq,
  };
  return hits.sort(by[filters.sort]);
}

const STATE_LABEL: Record<StateFilter, string> = { open: "Open", done: "Done", archived: "Archived", any: "Any" };
const SORT_LABEL: Record<SortKey, string> = { tracked: "last tracked", seq: "newest", title: "title", total: "total" };

function chip(active: boolean) {
  return cn(
    "flex h-7 items-center gap-[6px] rounded-2 border px-[9px] text-[12px] outline-none data-[state=open]:border-s2",
    active ? "border-accent text-link" : "border-s1 text-sub1 hover:border-s2 hover:bg-[color-mix(in_srgb,var(--c-text)_5%,transparent)] hover:text-fg",
  );
}

export function StateMenu({ value, onChange }: { value: StateFilter; onChange: (v: StateFilter) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className={chip(false)} aria-label="State filter">
        State <span className="text-fg">{STATE_LABEL[value]}</span>
        <CaretDown size={10} className="text-faint" />
      </DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuRadioGroup value={value} onValueChange={(v) => onChange(v as StateFilter)}>
          {(["open", "done", "archived", "any"] as const).map((s) => (
            <DropdownMenuRadioItem key={s} value={s}>
              {STATE_LABEL[s]}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function MultiMenu({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { id: Uuid; name: string }[];
  value: Uuid[];
  onChange: (v: Uuid[]) => void;
}) {
  const names = options.filter((o) => value.includes(o.id)).map((o) => o.name);
  const active = names.length > 0;
  return (
    <span className="flex items-center">
      <DropdownMenu>
        <DropdownMenuTrigger className={cn(chip(active), active && "rounded-r-none border-r-0 pr-1")} aria-label={`${label} filter`}>
          {label} <span className={active ? "max-w-[160px] truncate" : "text-faint"}>{active ? names.join(", ") : "any"}</span>
          {!active && <CaretDown size={10} className="text-faint" />}
        </DropdownMenuTrigger>
        <DropdownMenuContent className="tt-scroll max-h-[320px] overflow-y-auto">
          {options.length === 0 && <div className="px-[9px] py-1 text-[12px] text-muted">None yet</div>}
          {options.map((o) => (
            <DropdownMenuCheckboxItem
              key={o.id}
              checked={value.includes(o.id)}
              onCheckedChange={(checked) => onChange(checked ? [...value, o.id] : value.filter((v) => v !== o.id))}
            >
              {o.name}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      {active && (
        <button type="button" aria-label={`Clear ${label} filter`} onClick={() => onChange([])} className={cn(chip(true), "rounded-l-none border-l-0 px-[6px] opacity-70 hover:opacity-100")}>
          <X size={10} />
        </button>
      )}
    </span>
  );
}

export function SortMenu({ value, onChange }: { value: SortKey; onChange: (v: SortKey) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="text-[12px] text-faint hover:text-fg" aria-label="Sort">
        Sort · {SORT_LABEL[value]}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuRadioGroup value={value} onValueChange={(v) => onChange(v as SortKey)}>
          {(["tracked", "seq", "title", "total"] as const).map((s) => (
            <DropdownMenuRadioItem key={s} value={s}>
              {SORT_LABEL[s]}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
