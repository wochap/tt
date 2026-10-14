// Entry filter (Turn 8, timeline and Reports): a popover with search + Projects/Tags multi-select,
// the toolbar trigger with a count badge, and the removable chips row.

import { Funnel, X } from "@phosphor-icons/react";
import { useMemo, useState } from "react";

import { type Entry, type Range, totals, type Uuid, formatDuration } from "@tt/domain";

import { Command, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useView } from "@/data/react";
import { cssColor, NEUTRAL, projectColor } from "@/lib/colors";
import { cn } from "@/lib/utils";

import { filterActive, filterCount, NO_PROJECT, type EntryFilter } from "./filter.ts";

interface Option {
  kind: "project" | "tag";
  id: string;
  name: string;
  color: string;
  seconds: number;
}

const valueOf = (o: { kind: string; id: string }) => `${o.kind}:${o.id}`;

function useOptions(entries: Entry[], now: number, range: Range, filter: EntryFilter, query: string) {
  const view = useView();
  return useMemo(() => {
    const byProject = new Map<string, number>();
    const byTag = new Map<string, number>();
    for (const entry of entries) {
      const seconds = totals([entry], now, range).summed;
      const task = view.workspace.tasks.get(entry.task);
      const project = task?.project && view.workspace.projects.has(task.project) ? task.project : NO_PROJECT;
      byProject.set(project, (byProject.get(project) ?? 0) + seconds);
      for (const tag of task?.tags ?? []) byTag.set(tag, (byTag.get(tag) ?? 0) + seconds);
    }
    const q = query.trim().toLowerCase();
    const wanted = (name: string, id: string, time: Map<string, number>, selected: string[]) =>
      q ? name.toLowerCase().includes(q) : (time.get(id) ?? 0) > 0 || selected.includes(id);
    const order = (a: Option, b: Option) => b.seconds - a.seconds || a.name.localeCompare(b.name);
    const projects: Option[] = [
      { kind: "project" as const, id: NO_PROJECT, name: "No project", color: NEUTRAL, seconds: byProject.get(NO_PROJECT) ?? 0 },
      ...[...view.workspace.projects.values()].map((p) => ({ kind: "project" as const, id: p.id, name: p.name, color: projectColor(p), seconds: byProject.get(p.id) ?? 0 })),
    ]
      .filter((o) => wanted(o.name, o.id, byProject, filter.projects))
      .sort(order);
    const tags: Option[] = [...view.workspace.tags.values()]
      .map((t) => ({ kind: "tag" as const, id: t.id, name: t.name, color: cssColor(t.color) ?? NEUTRAL, seconds: byTag.get(t.id) ?? 0 }))
      .filter((o) => wanted(o.name, o.id, byTag, filter.tags))
      .sort(order);
    return { projects, tags };
  }, [view, entries, now, range, filter.projects, filter.tags, query]);
}

export function toggleOption(filter: EntryFilter, kind: "project" | "tag", id: string): EntryFilter {
  const key = kind === "project" ? "projects" : "tags";
  const list = filter[key];
  return { ...filter, [key]: list.includes(id) ? list.filter((x) => x !== id) : [...list, id] };
}

function FilterBody({
  filter,
  onChange,
  onClose,
  entries,
  now,
  range,
}: {
  filter: EntryFilter;
  onChange: (next: EntryFilter) => void;
  onClose: () => void;
  entries: Entry[];
  now: number;
  range: Range;
}) {
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState("");
  const { projects, tags } = useOptions(entries, now, range, filter, query);
  const all = [...projects, ...tags];
  const toggle = (option: Option) => onChange(toggleOption(filter, option.kind, option.id));

  const row = (option: Option) => {
    const on = (option.kind === "project" ? filter.projects : filter.tags).includes(option.id);
    return (
      <CommandItem
        key={valueOf(option)}
        value={valueOf(option)}
        data-checked={on || undefined}
        onSelect={() => {
          toggle(option);
          onClose();
        }}
      >
        <span className={cn("flex size-3 shrink-0 items-center justify-center rounded-xs border text-[9px] leading-none", on ? "border-accent bg-accent text-base" : "border-s2")}>{on ? "✓" : ""}</span>
        <span className="size-2 shrink-0 rounded-xs" style={{ background: option.color }} />
        <span className="min-w-0 flex-1 truncate">{option.kind === "tag" ? `+${option.name}` : option.name}</span>
        <span className="shrink-0 text-[11px] tabular-nums text-muted">{option.seconds > 0 ? formatDuration(option.seconds) : ""}</span>
      </CommandItem>
    );
  };

  return (
    <Command
      value={highlighted}
      onValueChange={setHighlighted}
      label="Filter timeline"
      onKeyDown={(event) => {
        if (event.key === " " && !query) {
          const option = all.find((o) => valueOf(o) === highlighted);
          if (option) {
            event.preventDefault();
            toggle(option);
          }
        } else if (event.key === "Backspace" && !query) {
          const tag = filter.tags[filter.tags.length - 1];
          const project = filter.projects[filter.projects.length - 1];
          if (tag !== undefined) onChange({ ...filter, tags: filter.tags.slice(0, -1) });
          else if (project !== undefined) onChange({ ...filter, projects: filter.projects.slice(0, -1) });
        }
      }}
    >
      <div className="flex h-[38px] items-center gap-2 border-b px-3 text-[13px]">
        <CommandInput autoFocus value={query} onValueChange={setQuery} placeholder="Filter by project or tag…" />
      </div>
      <CommandList className="max-h-[300px] text-[12.5px]">
        {projects.length > 0 && <CommandGroup heading="Projects">{projects.map(row)}</CommandGroup>}
        {tags.length > 0 && <CommandGroup heading="Tags">{tags.map(row)}</CommandGroup>}
        {all.length === 0 && <div className="px-3 py-3 text-[12px] text-muted">{query ? "No matching project or tag." : "Nothing tracked here — type to search."}</div>}
      </CommandList>
      <div className="flex gap-3 border-t px-3 py-[7px] text-[11px] text-muted">
        <span>
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
        </span>
        <span>
          <Kbd>Space</Kbd> toggle
        </span>
        <span>
          <Kbd>⏎</Kbd> toggle &amp; close
        </span>
        <span className="ml-auto">
          <Kbd>Esc</Kbd> close
        </span>
      </div>
    </Command>
  );
}

/** Toolbar trigger and popover. `F` opens it (controlled by the page). */
export function EntryFilterButton(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  filter: EntryFilter;
  onChange: (next: EntryFilter) => void;
  entries: Entry[];
  now: number;
  range: Range;
  hint?: boolean;
  size?: "sm" | "lg";
}) {
  const count = filterCount(props.filter);
  return (
    <Popover open={props.open} onOpenChange={props.onOpenChange}>
      <PopoverTrigger
        className={cn("tt-chip-btn", props.size === "lg" && "size-8 justify-center px-0", filterActive(props.filter) && "border-accent text-link")}
        aria-label={count ? `Filter, ${count} active` : "Filter"}
      >
        <Funnel size={13} />
        {props.size !== "lg" && "Filter"}
        {count > 0 && <span className="rounded-full bg-accent px-[5px] text-[10px] leading-4 text-base">{count}</span>}
        {props.hint && count === 0 && <Kbd>F</Kbd>}
      </PopoverTrigger>
      <PopoverContent className="w-[300px] overflow-hidden" data-testid="timeline-filter" align="end">
        {props.open && <FilterBody filter={props.filter} onChange={props.onChange} onClose={() => props.onOpenChange(false)} entries={props.entries} now={props.now} range={props.range} />}
      </PopoverContent>
    </Popover>
  );
}

/** Removable chips, Clear and the note on non-matching entries; renders nothing while no filter is active. */
export function EntryFilterChips({
  filter,
  onChange,
  className,
  note = "Other entries are dimmed and left out of totals",
}: {
  filter: EntryFilter;
  onChange: (next: EntryFilter) => void;
  className?: string;
  note?: string;
}) {
  const view = useView();
  if (!filterActive(filter)) return null;
  const chips: { kind: "project" | "tag"; id: Uuid; name: string; color: string }[] = [
    ...filter.projects.map((id) => {
      const project = view.workspace.projects.get(id);
      return { kind: "project" as const, id, name: project?.name ?? "No project", color: project ? projectColor(project) : NEUTRAL };
    }),
    ...filter.tags.map((id) => {
      const tag = view.workspace.tags.get(id);
      return { kind: "tag" as const, id, name: tag?.name ?? id, color: cssColor(tag?.color) ?? NEUTRAL };
    }),
  ];
  return (
    <div className={cn("flex min-h-8 flex-none flex-wrap items-center gap-2 border-b px-[14px] py-[5px] text-[12px]", className)} data-testid="filter-chips">
      {chips.map((chip) => (
        <span key={`${chip.kind}:${chip.id}`} className="flex h-6 items-center gap-[6px] rounded-2 border border-s1 pl-2 pr-1" data-testid="filter-chip">
          <span className="size-2 rounded-xs" style={{ background: chip.color }} />
          <span className="text-muted">{chip.kind}</span>
          <span>{chip.name}</span>
          <button type="button" aria-label={`Remove ${chip.kind} ${chip.name}`} className="flex size-4 items-center justify-center rounded-xs text-muted hover:text-fg" onClick={() => onChange(toggleOption(filter, chip.kind, chip.id))}>
            <X size={10} />
          </button>
        </span>
      ))}
      <button type="button" className="tt-chip-btn h-6" onClick={() => onChange({ projects: [], tags: [] })}>
        Clear <Kbd>⇧F</Kbd>
      </button>
      <span className="text-faint">{note}</span>
    </div>
  );
}
