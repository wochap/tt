import { Play, Stop } from "@phosphor-icons/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";

import { type FieldMatch, formatDuration, type Hit, type Task, type Uuid } from "@tt/domain";

import { Highlight, Seq, TagChips } from "@/components/task-bits";
import { ColorDot } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Segmented, SegmentedItem } from "@/components/ui/toggle-group";
import { useActions } from "@/data/actions";
import { useNow, useView } from "@/data/react";
import { runningTaskIds, taskTotals } from "@/data/select";
import { taskColor } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { usePhone } from "@/lib/media";
import { useTz } from "@/lib/settings";
import { MOD, cn } from "@/lib/utils";

import { useStatusHints } from "../shell/ui-state.tsx";
import { columnFor, todayIn } from "../timeline/geometry.ts";
import { Board } from "./Board.tsx";
import { DEFAULT_FILTERS, filterTasks, MultiMenu, SortMenu, StateMenu, type TaskFilters } from "./filters.tsx";
import { type QuickCreateHandle, QuickCreateInput } from "./QuickCreateInput.tsx";
import { RenumberHint, renumberHints } from "./renumber-hints.tsx";

export function TasksPage() {
  const view = useView();
  const actions = useActions();
  const navigate = useNavigate();
  const phone = usePhone();
  const tz = useTz();
  const now = useNow(30_000);
  const [params, setParams] = useSearchParams();
  const mode = params.get("view") === "board" && !phone ? "board" : "list";
  const [filters, setFilters] = useState<TaskFilters>(() => ({ ...DEFAULT_FILTERS, query: params.get("q") ?? "" }));
  const [focus, setFocus] = useState<Uuid>();
  const [renaming, setRenaming] = useState<Uuid>();
  const searchInput = useRef<HTMLInputElement>(null);
  const quick = useRef<QuickCreateHandle>(null);

  const today = columnFor(tz, todayIn(tz, now));
  const totalAll = useMemo(() => taskTotals(view, now), [view, now]);
  const totalToday = useMemo(() => taskTotals(view, now, { from: today.from, to: today.to }), [view, now, today.from, today.to]);
  const running = runningTaskIds(view);
  const { hits, elapsed } = useMemo(() => {
    const t0 = performance.now();
    const result = filterTasks(view, filters, totalAll, mode === "board");
    return { hits: result, elapsed: Math.max(1, Math.round(performance.now() - t0)) };
  }, [view, filters, totalAll, mode]);

  const hints = useMemo(() => renumberHints(view.workspace.tasks.values(), filters.query), [view, filters.query]);
  const holder = hints.length ? hits.findIndex((hit) => hit.task.seq === hints[0]!.from) : -1;
  const hintRows = hints.map((hint) => (
    <RenumberHint key={`hint-${hint.id}`} hint={hint} onOpen={() => navigate(`/tasks/${hint.to}`)} className="border-b px-[14px] pl-16" />
  ));

  const setMode = (next: "list" | "board") => {
    const q = new URLSearchParams(params);
    if (next === "board") q.set("view", "board");
    else q.delete("view");
    setParams(q, { replace: true });
  };

  const focusIndex = hits.findIndex((h) => h.task.id === focus);
  const focused = focusIndex >= 0 ? hits[focusIndex]!.task : undefined;
  useEffect(() => {
    if (focusIndex < 0 && hits.length && focus !== undefined) setFocus(hits[0]!.task.id);
  }, [focusIndex, hits, focus]);
  const move = (delta: number) => {
    if (!hits.length) return;
    const next = hits[Math.min(hits.length - 1, Math.max(0, (focusIndex < 0 ? -1 : focusIndex) + delta))]!;
    setFocus(next.task.id);
    document.querySelector(`[data-task-row="${next.task.id}"]`)?.scrollIntoView({ block: "nearest" });
  };

  useKeys({
    "j|arrowdown": () => move(1),
    "k|arrowup": () => move(-1),
    "/": () => searchInput.current?.focus(),
    enter: () => (focused ? void navigate(`/tasks/${focused.seq}`) : false),
    s: () => (focused ? actions.toggle(focused.id) : false),
    e: () => (focused ? setRenaming(focused.id) : false),
    x: () => (focused ? actions.setState(focused.id, focused.state === "done" ? "open" : "done") : false),
    b: () => setMode("board"),
    l: () => setMode("list"),
    n: () => quick.current?.focus(),
  });

  const counts = useMemo(() => {
    const c = { open: 0, done: 0, archived: 0 };
    for (const task of view.workspace.tasks.values()) c[task.state]++;
    return c;
  }, [view]);
  const countText = (["open", "done", "archived"] as const)
    .filter((s) => counts[s])
    .map((s) => `${counts[s]} ${s}`)
    .join(" · ");

  useStatusHints(<span>{view.workspace.tasks.size} tasks local</span>, [view.workspace.tasks.size]);

  const tagOptions = [...view.workspace.tags.values()].sort((a, b) => a.name.localeCompare(b.name));
  const projectOptions = [...view.workspace.projects.values()].filter((p) => !p.archived).sort((a, b) => a.name.localeCompare(b.name));
  const set = (patch: Partial<TaskFilters>) => setFilters((f) => ({ ...f, ...patch }));
  const empty = view.workspace.tasks.size === 0;

  const create = (input: Parameters<typeof actions.createTask>[0], start: boolean) => {
    const task = actions.createTask(input, { start });
    if (task) setFocus(task.id);
  };

  if (phone) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex flex-col gap-2 border-b px-[14px] pb-[10px] pt-[6px]">
          <QuickCreateInput ref={quick} compact placeholder="Quick create · title +tag @project" onCreate={(t) => create(t, false)} onCreateAndStart={(t) => create(t, true)} />
          <div className="flex gap-[6px] overflow-x-auto text-[12px]">
            <StateMenu value={filters.state} onChange={(state) => set({ state })} />
            <MultiMenu label="Tags" options={tagOptions} value={filters.tags} onChange={(tags) => set({ tags })} />
            <MultiMenu label="Project" options={projectOptions} value={filters.projects} onChange={(projects) => set({ projects })} />
            <input
              ref={searchInput}
              aria-label="Search tasks"
              value={filters.query}
              onChange={(e) => set({ query: e.target.value })}
              placeholder="Search"
              className="h-7 w-[110px] shrink-0 rounded-2 border border-s1 bg-transparent px-[10px] outline-none placeholder:text-muted focus:border-accent"
            />
          </div>
        </div>
        <div className="tt-scroll flex min-h-0 flex-1 flex-col overflow-y-auto">
          {hits.map(({ task }) => (
            <div key={task.id} className="flex min-h-[60px] items-center gap-[10px] border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] px-[14px] py-2" data-task-row={task.id}>
              <ColorDot color={taskColor(view.workspace, task)} />
              <button type="button" className="flex min-w-0 flex-1 flex-col gap-1 text-left" onClick={() => navigate(`/tasks/${task.seq}`)}>
                <span className="flex min-w-0 items-baseline gap-[6px]">
                  <Seq seq={task.seq} />
                  <span className={cn("truncate text-[13.5px]", task.state === "done" && "text-faint line-through")}>{task.title}</span>
                </span>
                <span className="flex flex-wrap items-center gap-1">
                  <TagChips view={view} task={task} />
                  <span className="text-[11px] text-faint">{task.project ? view.workspace.projects.get(task.project)?.name : ""}</span>
                  <span className="ml-auto text-[11px] tabular-nums text-faint">{totalToday.get(task.id) ? formatDuration(totalToday.get(task.id)!) : ""}</span>
                </span>
              </button>
              <PlayButton task={task} running={running.has(task.id)} size={44} />
            </div>
          ))}
          {empty && <EmptyTasks onStart={() => quick.current?.focus()} />}
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-col gap-[10px] border-b px-[14px] pb-[10px] pt-[14px]">
        <QuickCreateInput ref={quick} autoFocus={empty} onCreate={(t) => create(t, false)} onCreateAndStart={(t) => create(t, true)} />
        <div className="flex flex-wrap items-center gap-2 text-[12px]">
          <label className="flex h-7 w-[260px] items-center gap-2 rounded-2 border border-s1 bg-base px-[9px] focus-within:border-accent">
            <input
              ref={searchInput}
              aria-label="Search tasks"
              value={filters.query}
              onChange={(e) => set({ query: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  set({ query: "" });
                  searchInput.current?.blur();
                } else if (e.key === "Enter" || e.key === "ArrowDown") {
                  e.preventDefault();
                  searchInput.current?.blur();
                  if (hits[0]) setFocus(hits[0].task.id);
                }
              }}
              placeholder="Fuzzy search · title, tags, metadata"
              className={cn("min-w-0 flex-1 bg-transparent outline-none placeholder:text-faint", filters.query && "font-mono")}
            />
            {filters.query ? (
              <span className="shrink-0 text-muted" data-testid="search-count">
                {hits.length} match{hits.length === 1 ? "" : "es"} · {elapsed} ms
              </span>
            ) : (
              <Kbd>/</Kbd>
            )}
          </label>
          {mode === "list" && <StateMenu value={filters.state} onChange={(state) => set({ state })} />}
          <MultiMenu label="Tags" options={tagOptions} value={filters.tags} onChange={(tags) => set({ tags })} />
          <MultiMenu label="Project" options={projectOptions} value={filters.projects} onChange={(projects) => set({ projects })} />
          {filters.query && <span className="ml-2 text-muted">Matched in: title · tags · <span className="text-link">metadata values</span></span>}
          <span className="ml-auto flex items-center gap-3">
            <Segmented label="View" value={mode} onValueChange={setMode}>
              <SegmentedItem value="list" className="px-[9px] py-1">
                List <Kbd>L</Kbd>
              </SegmentedItem>
              <SegmentedItem value="board" className="px-[9px] py-1">
                Board <Kbd>B</Kbd>
              </SegmentedItem>
            </Segmented>
            {mode === "list" && <SortMenu value={filters.sort} onChange={(sort) => set({ sort })} />}
          </span>
        </div>
      </div>
      {mode === "board" ? (
        <Board hits={hits} totals={totalAll} focus={focus} setFocus={setFocus} />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <div
            className="grid gap-[10px] border-b px-[14px] py-[6px] text-[10.5px] uppercase tracking-[.08em] text-muted"
            style={{ gridTemplateColumns: COLUMNS(!!filters.query) }}
          >
            <span>#</span>
            <span>Task</span>
            <span>Tags</span>
            <span>Metadata</span>
            <span className="text-right">Today</span>
            <span className="text-right">Total</span>
            <span />
          </div>
          <div className="tt-scroll min-h-0 flex-1 overflow-y-auto" role="list" aria-label="Tasks">
            {hits.map((hit, index) => [
              <TaskRow
                key={hit.task.id}
                hit={hit}
                searching={!!filters.query}
                focused={hit.task.id === focus}
                running={running.has(hit.task.id)}
                today={totalToday.get(hit.task.id)}
                total={totalAll.get(hit.task.id)}
                renaming={renaming === hit.task.id}
                onRenamed={() => setRenaming(undefined)}
                onFocus={() => setFocus(hit.task.id)}
              />,
              index === holder && hintRows,
            ])}
            {holder < 0 && hintRows}
            {empty ? (
              <EmptyTasks onStart={() => quick.current?.focus()} />
            ) : (
              <div className="flex gap-[14px] px-[14px] py-[10px] text-[11.5px] text-faint">
                <span>{countText}</span>
                <span>
                  <Kbd>J</Kbd>
                  <Kbd>K</Kbd> move
                </span>
                <span>
                  <Kbd>S</Kbd> start
                </span>
                <span>
                  <Kbd>⏎</Kbd> open
                </span>
                <span>
                  <Kbd>E</Kbd> edit title
                </span>
                <span>
                  <Kbd>X</Kbd> done
                </span>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

const COLUMNS = (searching: boolean) => `40px minmax(0,1fr) 230px ${searching ? 160 : 110}px 80px 80px 32px`;

function metadataCell(task: Task, matches: FieldMatch[]) {
  const hit = matches.find((m) => m.kind === "meta");
  if (hit) {
    return (
      <>
        {hit.key}: <Highlight text={hit.text} indices={hit.indices} mark="yellow" />
      </>
    );
  }
  const [key, value] = Object.entries(task.metadata)[0] ?? [];
  return key ? `${key}: ${value}` : "";
}

function TaskRow({
  hit,
  searching,
  focused,
  running,
  today,
  total,
  renaming,
  onRenamed,
  onFocus,
}: {
  hit: Hit;
  searching: boolean;
  focused: boolean;
  running: boolean;
  today?: number;
  total?: number;
  renaming: boolean;
  onRenamed: () => void;
  onFocus: () => void;
}) {
  const view = useView();
  const actions = useActions();
  const navigate = useNavigate();
  const { task, matches } = hit;
  const project = task.project ? view.workspace.projects.get(task.project) : undefined;
  const done = task.state === "done";
  const title = matches.find((m) => m.kind === "title");
  return (
    <div
      role="listitem"
      data-task-row={task.id}
      data-testid="task-row"
      onClick={onFocus}
      onDoubleClick={() => navigate(`/tasks/${task.seq}`)}
      className={cn(
        "group grid h-[38px] items-center gap-[10px] border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] px-[14px] text-[13px]",
        focused ? "tt-row-focus" : "tt-row-hover",
        done && "opacity-60",
      )}
      style={{ gridTemplateColumns: COLUMNS(searching) }}
    >
      <span className="font-mono text-[11px] text-faint">#{task.seq}</span>
      <span className="flex min-w-0 items-center gap-2">
        <ColorDot color={taskColor(view.workspace, task)} />
        {renaming ? (
          <RenameInput task={task} onDone={onRenamed} />
        ) : (
          <button
            type="button"
            className={cn("truncate text-left hover:underline", done && "text-faint line-through")}
            onClick={(e) => {
              e.stopPropagation();
              void navigate(`/tasks/${task.seq}`);
            }}
          >
            <Highlight text={task.title} indices={title?.indices} />
          </button>
        )}
        {project && <span className="shrink-0 text-[11px] text-faint">{project.name}</span>}
        {running && (
          <span className="flex shrink-0 items-center gap-1 text-[10.5px] text-green-fg">
            <span className="size-[6px] rounded-full bg-green" />
            running
          </span>
        )}
      </span>
      <span className="flex min-w-0 flex-wrap gap-1 overflow-hidden">
        <TagChips view={view} task={task} />
      </span>
      <span className="truncate font-mono text-[11px] text-muted">{metadataCell(task, matches)}</span>
      <span className="text-right tabular-nums text-sub1">{today ? formatDuration(today) : ""}</span>
      <span className="text-right tabular-nums">{total ? formatDuration(total) : "—"}</span>
      <PlayButton task={task} running={running} focused={focused} onToggle={() => actions.toggle(task.id)} />
    </div>
  );
}

function RenameInput({ task, onDone }: { task: Task; onDone: () => void }) {
  const actions = useActions();
  const [value, setValue] = useState(task.title);
  const commit = () => {
    if (value.trim() && value.trim() !== task.title) actions.updateTask(task.id, { title: value }, `Renamed #${task.seq}`);
    onDone();
  };
  return (
    <input
      autoFocus
      aria-label="Task title"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") onDone();
      }}
      className="min-w-0 flex-1 rounded-sm border border-accent bg-mantle px-1 outline-none"
    />
  );
}

export function PlayButton({ task, running, focused, size = 24, onToggle }: { task: Task; running: boolean; focused?: boolean; size?: number; onToggle?: () => void }) {
  const actions = useActions();
  return (
    <button
      type="button"
      aria-label={running ? `Stop ${task.title}` : `Start ${task.title}`}
      onClick={(e) => {
        e.stopPropagation();
        if (onToggle) onToggle();
        else actions.toggle(task.id);
      }}
      className={cn(
        "flex shrink-0 items-center justify-center border",
        size > 30 ? "rounded-frame" : "rounded-sm",
        running
          ? "border-[color-mix(in_srgb,var(--ctp-green)_45%,transparent)] text-green-fg"
          : focused
            ? "border-accent text-link"
            : "border-transparent text-faint group-hover:border-s1 group-hover:text-link",
        size > 30 && !running && "border-s1 text-link",
      )}
      style={{ width: size, height: size }}
    >
      {running ? <Stop size={size > 30 ? 12 : 9} weight="fill" /> : <Play size={size > 30 ? 12 : 9} weight="fill" />}
    </button>
  );
}

function EmptyTasks({ onStart }: { onStart: () => void }) {
  return (
    <div className="flex flex-col gap-[14px] px-[14px] py-3" data-testid="empty-tasks">
      <div className="flex flex-col gap-[6px] text-[12px] leading-[1.5] text-muted">
        <div className="text-[13px] font-medium text-fg">No tasks yet</div>
        <div>
          Type a title and press <Kbd>⏎</Kbd>. Add tags with <span className="font-mono text-fg">+tag</span>, a project with{" "}
          <span className="font-mono text-fg">@project</span>, metadata with <span className="font-mono text-fg">key:value</span>. <Kbd>{MOD}⏎</Kbd>{" "}
          creates and starts tracking.
        </div>
      </div>
      <div className="flex gap-2">
        <Button variant="primary" size="sm" onClick={onStart}>
          Create &amp; start
        </Button>
      </div>
    </div>
  );
}
