import { CaretLeft, CaretRight, Play, X } from "@phosphor-icons/react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";

import {
  addDays,
  type Entry,
  entryDuration,
  formatClock,
  formatDate,
  formatDayShort,
  formatDuration,
  formatElapsed,
  localDate,
  type Task,
  TASK_STATES,
  type TaskState,
  taskProject,
  taskTags,
  weekStartDate,
} from "@tt/domain";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Segmented, SegmentedItem } from "@/components/ui/toggle-group";
import { useActions } from "@/data/actions";
import { useNow, useView } from "@/data/react";
import { cssTextColor, projectColor, projectColorName, tagColor } from "@/lib/colors";
import { useKeys } from "@/lib/keys";
import { useSettings, useTz } from "@/lib/settings";
import { MOD, cn } from "@/lib/utils";

import { useStatusHints } from "../shell/ui-state.tsx";
import { columnFor } from "../timeline/geometry.ts";

const STATE_LABEL: Record<TaskState, string> = { open: "Open", done: "Done", archived: "Archive" };

/** Frames 9 and 18: one task, its metadata, description and entries. */
export function TaskDetailPage() {
  const { seq } = useParams();
  const view = useView();
  const navigate = useNavigate();
  const task = [...view.workspace.tasks.values()].find((t) => String(t.seq) === seq?.replace(/^#/, ""));
  const ordered = useMemo(() => [...view.workspace.tasks.values()].filter((t) => t.state !== "archived" || t.seq === task?.seq).sort((a, b) => a.seq - b.seq), [view, task?.seq]);
  const index = task ? ordered.findIndex((t) => t.id === task.id) : -1;
  const step = (delta: number) => {
    const next = ordered[index + delta];
    if (next) void navigate(`/tasks/${next.seq}`);
  };
  useKeys({ j: () => step(1), k: () => step(-1) });
  useStatusHints(
    <>
      <span><Kbd>J</Kbd><Kbd>K</Kbd> next / previous task</span>
      <span><Kbd>S</Kbd> start</span>
      <span><Kbd>E</Kbd> edit title</span>
      <span><Kbd>{MOD}E</Kbd> description</span>
    </>,
    [],
  );

  if (!task) {
    return (
      <div className="m-auto text-center text-[13px] text-muted">
        No task #{seq}. <Link to="/tasks" className="text-link">Back to tasks</Link>
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-11 flex-none items-center gap-3 border-b bg-mantle px-[14px] text-[12.5px] text-muted">
        <Link to="/tasks" className="flex items-center gap-1 text-link">
          <CaretLeft size={11} /> Tasks
        </Link>
        <span className="text-faint">/</span>
        <span className="font-mono text-fg">#{task.seq}</span>
        {task.previousSeqs?.map((n) => (
          <span key={n} className="text-[11.5px] text-faint">
            previously <span className="font-mono">#{n}</span>
          </span>
        ))}
        <span className="ml-auto flex items-center gap-[6px]">
          <button type="button" aria-label="Previous task" className="tt-icon-btn size-6" disabled={index <= 0} onClick={() => step(-1)}>
            <CaretLeft size={11} />
          </button>
          <button type="button" aria-label="Next task" className="tt-icon-btn size-6" disabled={index >= ordered.length - 1} onClick={() => step(1)}>
            <CaretRight size={11} />
          </button>
          <Kbd>J</Kbd>
          <Kbd>K</Kbd> next / previous task
        </span>
      </div>
      <TaskDetail key={task.id} task={task} />
    </div>
  );
}

function TaskDetail({ task }: { task: Task }) {
  const actions = useActions();
  const now = useNow(1000);
  const [editingTitle, setEditingTitle] = useState(false);
  const running = actions.runningFor(task.id);
  const elapsed = running.reduce((max, e) => Math.max(max, entryDuration(e, now)), 0);

  useKeys({
    s: () => actions.toggle(task.id),
    e: () => setEditingTitle(true),
    x: () => actions.setState(task.id, task.state === "done" ? "open" : "done"),
  });

  return (
    <div className="tt-scroll flex min-h-0 flex-1 flex-col gap-[18px] overflow-y-auto px-[22px] py-[18px]">
      <div className="mx-auto flex w-full max-w-[1000px] flex-col gap-[18px]">
        <div className="flex items-start gap-3">
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <TitleEditor task={task} editing={editingTitle} setEditing={setEditingTitle} />
            <div className="flex flex-wrap items-center gap-[6px] text-[12px]">
              <ProjectChip task={task} />
              <TagEditor task={task} />
              <Segmented
                label="State"
                className="ml-auto"
                value={task.state}
                onValueChange={(state: TaskState) => actions.setState(task.id, state)}
              >
                {TASK_STATES.map((s) => (
                  <SegmentedItem key={s} value={s} className="px-[10px] py-[3px]">
                    {STATE_LABEL[s]}
                  </SegmentedItem>
                ))}
              </Segmented>
            </div>
          </div>
          {running.length ? (
            <Button variant="running" className="flex-none gap-2" onClick={() => actions.toggle(task.id)} data-testid="task-toggle">
              <span className="size-2 rounded-xs bg-current" />
              Stop {formatElapsed(elapsed)}
            </Button>
          ) : (
            <Button variant="primary" className="flex-none gap-2" onClick={() => actions.toggle(task.id)} data-testid="task-toggle">
              <Play size={11} weight="fill" /> Start tracking <Kbd inherit>S</Kbd>
            </Button>
          )}
        </div>
        <div className="grid grid-cols-1 gap-[18px] md:grid-cols-2">
          <MetadataEditor task={task} />
          <DescriptionEditor task={task} />
        </div>
        <EntriesList task={task} now={now} />
      </div>
    </div>
  );
}

function TitleEditor({ task, editing, setEditing }: { task: Task; editing: boolean; setEditing: (v: boolean) => void }) {
  const actions = useActions();
  const [value, setValue] = useState(task.title);
  useEffect(() => setValue(task.title), [task.title]);
  const commit = () => {
    setEditing(false);
    if (value.trim() && value.trim() !== task.title) actions.updateTask(task.id, { title: value }, `Renamed #${task.seq}`);
    else setValue(task.title);
  };
  if (editing) {
    return (
      <input
        autoFocus
        aria-label="Title"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            setValue(task.title);
            setEditing(false);
          }
        }}
        className="border-b border-dashed border-accent bg-transparent py-[2px] text-[22px] font-medium leading-[1.2] tracking-[-.02em] outline-none"
      />
    );
  }
  return (
    <h1
      className="cursor-text border-b border-dashed border-transparent py-[2px] text-[22px] font-medium leading-[1.2] tracking-[-.02em] hover:border-s2"
      onClick={() => setEditing(true)}
      data-testid="task-title"
    >
      {task.title}
    </h1>
  );
}

function ProjectChip({ task }: { task: Task }) {
  const view = useView();
  const actions = useActions();
  const project = taskProject(view.workspace, task);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const names = [...view.workspace.projects.values()].filter((p) => !p.archived).map((p) => p.name);
  if (editing) {
    return (
      <span className="flex items-center gap-1">
        <input
          autoFocus
          list="tt-projects"
          aria-label="Project"
          value={value}
          placeholder="project"
          onChange={(e) => setValue(e.target.value)}
          onBlur={() => setEditing(false)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              actions.updateTask(task.id, { project: value.trim() ? value.trim() : null }, value.trim() ? `Moved #${task.seq} to ${value.trim()}` : `Cleared project of #${task.seq}`);
              setEditing(false);
            }
            if (e.key === "Escape") setEditing(false);
          }}
          className="w-[140px] rounded-2 border border-accent bg-mantle px-2 py-[2px] outline-none"
        />
        <datalist id="tt-projects">
          {names.map((n) => (
            <option key={n} value={n} />
          ))}
        </datalist>
      </span>
    );
  }
  if (!project) {
    return (
      <button type="button" onClick={() => (setValue(""), setEditing(true))} className="rounded-2 border border-dashed border-s2 px-[9px] py-[3px] text-faint hover:text-fg">
        + project
      </button>
    );
  }
  const color = projectColor(project);
  return (
    <Badge size="md" color={color} textColor={cssTextColor(projectColorName(project))} strength={18} className="cursor-pointer" onClick={() => (setValue(project.name), setEditing(true))}>
      <span className="size-[7px] rounded-xs" style={{ background: color }} />
      {project.name}
    </Badge>
  );
}

function TagEditor({ task }: { task: Task }) {
  const view = useView();
  const actions = useActions();
  const [adding, setAdding] = useState(false);
  const [value, setValue] = useState("");
  const tags = taskTags(view.workspace, task);
  return (
    <>
      {tags.map((tag) => (
        <Badge key={tag.id} size="md" color={tagColor(tag)} textColor={cssTextColor(tag.color)} className="group/tag gap-1">
          {tag.name}
          <button
            type="button"
            aria-label={`Remove tag ${tag.name}`}
            onClick={() => actions.updateTask(task.id, { removeTags: [tag.name] }, `Removed +${tag.name}`)}
            className="opacity-0 group-hover/tag:opacity-70 hover:!opacity-100 focus-visible:opacity-100"
          >
            <X size={9} />
          </button>
        </Badge>
      ))}
      {adding ? (
        <input
          autoFocus
          aria-label="New tag"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onBlur={() => setAdding(false)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && value.trim()) {
              actions.updateTask(task.id, { addTags: value.split(/[\s,]+/).filter(Boolean) }, `Tagged #${task.seq}`);
              setValue("");
            }
            if (e.key === "Escape") setAdding(false);
          }}
          placeholder="tag"
          className="w-[100px] rounded-2 border border-accent bg-mantle px-2 py-[2px] outline-none"
        />
      ) : (
        <button type="button" onClick={() => setAdding(true)} className="rounded-2 border border-dashed border-s2 px-[9px] py-[3px] text-faint hover:text-fg">
          + tag
        </button>
      )}
    </>
  );
}

function MetadataEditor({ task }: { task: Task }) {
  const actions = useActions();
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const keyInput = useRef<HTMLInputElement>(null);
  const add = () => {
    if (!key.trim() || !value.trim()) return;
    actions.updateTask(task.id, { setMetadata: { [key.trim()]: value.trim() } }, `Set ${key.trim()} on #${task.seq}`);
    setKey("");
    setValue("");
    keyInput.current?.focus();
  };
  return (
    <div className="flex flex-col gap-[6px]">
      <div className="tt-label">Metadata</div>
      <div className="grid grid-cols-[90px_1fr] gap-px overflow-hidden rounded-md border border-s1 bg-s1 font-mono text-[12.5px]" data-testid="metadata">
        {Object.entries(task.metadata).map(([k, v]) => (
          <MetadataRow key={k} task={task} name={k} value={v} />
        ))}
        <input
          ref={keyInput}
          aria-label="Metadata key"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder="key"
          className="bg-mantle px-[10px] py-[6px] text-muted outline-none placeholder:text-faint focus:text-fg"
        />
        <span className="flex items-center bg-base">
          <input
            aria-label="Metadata value"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && add()}
            placeholder="value"
            className="min-w-0 flex-1 bg-transparent px-[10px] py-[6px] outline-none placeholder:font-sans placeholder:text-faint"
          />
          {!key && !value && (
            <span className="pr-[10px] font-sans text-[11px] text-faint">
              <Kbd>⏎</Kbd> adds a row
            </span>
          )}
        </span>
      </div>
    </div>
  );
}

function MetadataRow({ task, name, value }: { task: Task; name: string; value: string }) {
  const actions = useActions();
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  const isUrl = /^https?:\/\//.test(value);
  const commit = () => {
    if (text === value) return;
    if (!text.trim()) actions.updateTask(task.id, { removeMetadata: [name] }, `Removed ${name} from #${task.seq}`);
    else actions.updateTask(task.id, { setMetadata: { [name]: text } }, `Set ${name} on #${task.seq}`);
  };
  return (
    <>
      <span className="group/meta flex items-center justify-between bg-mantle px-[10px] py-[6px] text-muted">
        {name}
        <button type="button" aria-label={`Remove ${name}`} onClick={() => actions.updateTask(task.id, { removeMetadata: [name] }, `Removed ${name} from #${task.seq}`)} className="opacity-0 group-hover/meta:opacity-70 hover:!opacity-100">
          <X size={9} />
        </button>
      </span>
      <span className="flex min-w-0 items-center bg-base">
        <input
          aria-label={name}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
          className={cn("min-w-0 flex-1 truncate bg-transparent px-[10px] py-[6px] outline-none", isUrl && "text-link")}
        />
        {isUrl && (
          <a href={value} target="_blank" rel="noreferrer noopener" className="pr-[10px] font-sans text-[11px] text-link">
            open
          </a>
        )}
      </span>
    </>
  );
}

function DescriptionEditor({ task }: { task: Task }) {
  const actions = useActions();
  const [mode, setMode] = useState<"edit" | "preview">(task.description ? "preview" : "edit");
  const [text, setText] = useState(task.description);
  useEffect(() => setText(task.description), [task.description]);
  const commit = () => text !== task.description && actions.updateTask(task.id, { description: text }, `Edited description of #${task.seq}`);
  useKeys(
    {
      "mod+e": () => {
        commit();
        setMode((m) => (m === "edit" ? "preview" : "edit"));
      },
    },
    { inInputs: true },
  );
  const html = useMemo(() => DOMPurify.sanitize(marked.parse(text || "", { async: false, gfm: true, breaks: true }) as string), [text]);
  return (
    <div className="flex flex-col gap-[6px]">
      <div className="tt-label flex items-center justify-between">
        <span>Description</span>
        <span className="flex normal-case tracking-normal">
          <button type="button" onClick={() => setMode("edit")} className={cn("px-[6px]", mode === "edit" ? "text-link" : "text-faint hover:text-fg")}>
            Edit
          </button>
          <button
            type="button"
            onClick={() => (commit(), setMode("preview"))}
            className={cn("border-l border-s1 px-[6px]", mode === "preview" ? "text-link" : "text-faint hover:text-fg")}
          >
            Preview <Kbd>{MOD}E</Kbd>
          </button>
        </span>
      </div>
      {mode === "edit" ? (
        <textarea
          aria-label="Description"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          placeholder="Markdown: ## headings, - [ ] lists, `code`"
          className="min-h-[118px] resize-y rounded-md border border-accent bg-mantle px-3 py-[10px] font-mono text-[12px] leading-[1.55] outline-none placeholder:text-faint"
        />
      ) : (
        <div
          className="tt-markdown min-h-[118px] rounded-md border border-s1 bg-mantle px-3 py-[10px] text-[12.5px] leading-[1.5]"
          onDoubleClick={() => setMode("edit")}
          data-testid="description-preview"
          dangerouslySetInnerHTML={{ __html: html || '<p class="text-faint">No description. Double-click to write one.</p>' }}
        />
      )}
    </div>
  );
}

function EntriesList({ task, now }: { task: Task; now: number }) {
  const view = useView();
  const tz = useTz();
  const { weekStart } = useSettings();
  const navigate = useNavigate();
  const [scope, setScope] = useState<"week" | "all">("week");
  const [limit, setLimit] = useState(5);
  const first = weekStartDate(localDate(tz, now), weekStart);
  const week = columnFor(tz, first);
  const weekEnd = columnFor(tz, addDays(first, 7)).from;
  const all = [...view.entries.values()].filter((e) => e.task === task.id).sort((a, b) => b.start - a.start);
  const inWeek = all.filter((e) => (e.end ?? now) > week.from && e.start < weekEnd);
  const shown = scope === "week" ? inWeek : all;
  const sum = (list: Entry[]) => list.reduce((acc, e) => acc + entryDuration(e, now), 0);
  return (
    <div className="flex min-h-0 flex-col gap-[6px]" data-testid="task-entries">
      <div className="tt-label flex items-center justify-between">
        <span className="flex items-center gap-[10px]">
          Entries
          <Segmented label="Entries scope" value={scope} onValueChange={(v: "week" | "all") => setScope(v)} className="text-[11px] normal-case tracking-normal">
            <SegmentedItem value="week" className="px-2 py-[2px]">
              This week
            </SegmentedItem>
            <SegmentedItem value="all" className="px-2 py-[2px]">
              All time
            </SegmentedItem>
          </Segmented>
        </span>
        <span className="text-[12.5px] normal-case tracking-normal text-fg">
          {formatDuration(sum(shown))}{" "}
          <span className="text-muted">
            · {shown.length} entr{shown.length === 1 ? "y" : "ies"}
            {scope === "week" && ` · ${formatDuration(sum(all))} all time`}
          </span>
        </span>
      </div>
      {shown.slice(0, limit).map((entry) => {
        const day = localDate(tz, entry.start);
        return (
          <button
            type="button"
            key={entry.id}
            onClick={() => navigate(`/timeline/day?date=${formatDate(day)}&entry=${entry.id}`)}
            className="tt-row-hover grid h-[34px] grid-cols-[110px_1fr_70px_24px] items-center gap-3 border-b border-[color-mix(in_srgb,var(--c-s0)_60%,transparent)] px-[6px] text-left text-[12.5px] tabular-nums"
          >
            <span className="text-sub1">{formatDayShort(day)}</span>
            <span className="flex items-center gap-2">
              {formatClock(tz, entry.start)} – {entry.end === null ? "now" : formatClock(tz, entry.end)}
              {entry.end === null && (
                <span className="flex items-center gap-1 text-[10.5px] text-green-fg">
                  <span className="size-[6px] rounded-full bg-green" />
                  running
                </span>
              )}
              {entry.note && <span className="truncate text-[11px] text-faint">{entry.note}</span>}
            </span>
            <span className="text-right">{formatDuration(entryDuration(entry, now))}</span>
            <span className="text-center text-faint">›</span>
          </button>
        );
      })}
      {shown.length > limit && (
        <button type="button" onClick={() => setLimit(limit + 20)} className="self-start px-[6px] py-[6px] text-[12px] text-muted hover:text-fg">
          Show {Math.min(20, shown.length - limit)} more…
        </button>
      )}
      {shown.length === 0 && <div className="px-[6px] py-2 text-[12px] text-muted">{scope === "week" ? "Nothing tracked this week." : "Nothing tracked yet."}</div>}
      <div className="pt-[6px] text-[11.5px] text-faint">Click an entry to open it on the timeline. Renaming this task renames all of them.</div>
    </div>
  );
}
