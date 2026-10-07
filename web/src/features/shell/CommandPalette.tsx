import { CaretRight } from "@phosphor-icons/react";
import { Dialog as D } from "radix-ui";
import { type ReactNode, useMemo, useState } from "react";
import { useNavigate } from "react-router";

import {
  entryDuration,
  formatDate,
  formatDayShort,
  formatElapsed,
  localDate,
  parseJumpDate,
  parseQuick,
  search,
  type Task,
  taskProject,
} from "@tt/domain";

import { Highlight } from "@/components/task-bits";
import { Command, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import { useActions } from "@/data/actions";
import { useNow, useView } from "@/data/react";
import { pickerTasks, runningTaskIds } from "@/data/select";
import { taskColor } from "@/lib/colors";
import { useSettings, useTz } from "@/lib/settings";
import { MOD } from "@/lib/utils";

import { useUi } from "./ui-state.tsx";

interface Row {
  id: string;
  kind: string;
  label: ReactNode;
  hint?: ReactNode;
  kbd?: string;
  color?: string;
  run: () => void;
  /** ⌘⏎: start and open (tasks). */
  alt?: () => void;
}

/** ⌘K: start (fuzzy over tasks), stop, jump to date, switch view, create task, settings. */
export function CommandPalette() {
  const { palette, closePalette } = useUi();
  return (
    <D.Root open={palette.open} onOpenChange={(open) => !open && closePalette()}>
      <D.Portal>
        <D.Overlay className="fixed inset-0 z-50 bg-[color-mix(in_srgb,var(--c-crust)_70%,transparent)]" />
        <D.Content
          aria-label="Command palette"
          className="fixed left-1/2 top-[72px] z-50 w-[min(560px,calc(100vw-24px))] -translate-x-1/2 overflow-hidden rounded-frame bg-mantle shadow-[var(--shadow-lg)] outline-none"
        >
          <D.Title className="sr-only">Command palette</D.Title>
          {palette.open && <PaletteBody key={palette.mode + palette.query} initial={palette.query} mode={palette.mode} close={closePalette} />}
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}

function PaletteBody({ initial, mode, close }: { initial: string; mode: "all" | "create"; close: () => void }) {
  const [query, setQuery] = useState(initial);
  const [selected, setSelected] = useState("");
  const view = useView();
  const actions = useActions();
  const navigate = useNavigate();
  const now = useNow(1000);
  const tz = useTz();
  const { weekStart } = useSettings();
  const running = runningTaskIds(view);

  const rows = useMemo<Row[]>(() => {
    const out: Row[] = [];
    const q = query.trim();
    const describe = (task: Task) => {
      const project = taskProject(view.workspace, task)?.name;
      const ticket = Object.values(task.metadata)[0];
      return [`#${task.seq}`, project, running.has(task.id) ? "running" : ticket].filter(Boolean).join(" · ");
    };
    const create: Row | undefined = q
      ? {
          id: "create",
          kind: "New",
          label: `Create task “${parseQuick(q).title || q}”`,
          hint: "+tags @project key:value",
          kbd: "C",
          run: () => void actions.createTask(parseQuick(q)),
          alt: () => {
            actions.createTask(parseQuick(q), { start: true });
          },
        }
      : undefined;
    if (mode === "create") return create ? [create] : [];

    // Stop: running entries matching the query.
    const runningEntries = [...view.entries.values()].filter((e) => e.end === null);
    const runningHits = q ? new Set(search(view, q, (t) => running.has(t.id)).map((h) => h.task.id)) : undefined;
    for (const entry of runningEntries) {
      const task = view.workspace.tasks.get(entry.task);
      if (runningHits && !runningHits.has(entry.task)) continue;
      out.push({
        id: `stop-${entry.id}`,
        kind: "Stop",
        label: task?.title ?? "(deleted task)",
        hint: `${task ? `#${task.seq} · ` : ""}running ${formatElapsed(entryDuration(entry, now)).replace(/:\d\d$/, "")}`,
        color: taskColor(view.workspace, task),
        run: () => actions.stop(entry.id),
      });
    }

    // Start: fuzzy over title, #seq, tags, metadata values, project.
    const hits = q
      ? search(view, q, (t) => t.state !== "archived").slice(0, 8)
      : pickerTasks(view)
          .filter((t) => !running.has(t.id))
          .slice(0, 5)
          .map((task) => ({ task, score: 0, matches: [] }));
    for (const hit of hits) {
      const titleMatch = hit.matches.find((m) => m.kind === "title");
      const metaMatch = hit.matches.find((m) => m.kind === "meta");
      out.push({
        id: `start-${hit.task.id}`,
        kind: "Start",
        label: <Highlight text={hit.task.title} indices={titleMatch?.indices} />,
        hint: metaMatch ? (
          <span className="font-mono">
            #{hit.task.seq} · <Highlight text={metaMatch.text} indices={metaMatch.indices} mark="yellow" />
          </span>
        ) : (
          describe(hit.task)
        ),
        color: taskColor(view.workspace, hit.task),
        run: () => actions.start(hit.task.id),
        alt: () => {
          actions.start(hit.task.id);
          navigate(`/tasks/${hit.task.seq}`);
        },
      });
    }

    // Go: a parsed date, or the jump popover.
    const today = localDate(tz, now);
    let parsed: string | undefined;
    if (q) {
      try {
        parsed = formatDate(parseJumpDate(q, today, weekStart));
      } catch {
        parsed = undefined;
      }
    }
    const commands: Row[] = [
      ...(parsed
        ? [
            {
              id: "go-date",
              kind: "Go",
              label: `Go to ${formatDayShort(parseJumpDate(q, today, weekStart))}`,
              hint: parsed,
              run: () => navigate(`/timeline/day?date=${parsed}`),
            },
          ]
        : []),
      { id: "jump", kind: "Go", label: "Jump to date…", hint: "tomorrow, 12 oct, w42", kbd: "G", run: () => navigate("/timeline/day?jump=1") },
      { id: "view-day", kind: "View", label: "Switch to day", kbd: "D", run: () => navigate("/timeline/day") },
      { id: "view-week", kind: "View", label: "Switch to week", kbd: "W", run: () => navigate("/timeline/week") },
      { id: "view-month", kind: "View", label: "Switch to month", kbd: "M", run: () => navigate("/timeline/month") },
      { id: "view-tasks", kind: "View", label: "Tasks · list", kbd: "2", run: () => navigate("/tasks") },
      { id: "view-board", kind: "View", label: "Tasks · board", run: () => navigate("/tasks?view=board") },
      { id: "view-reports", kind: "View", label: "Reports", kbd: "3", run: () => navigate("/reports") },
      { id: "stop-all", kind: "Stop", label: "Stop all running", kbd: "⇧S", run: () => actions.stopAll() },
      { id: "settings", kind: "App", label: "Settings", kbd: ",", run: () => navigate("/settings") },
      { id: "undo", kind: "App", label: "Undo", kbd: `${MOD}Z`, run: () => void actions.undo() },
    ];
    const lower = q.toLowerCase();
    const matching = q
      ? commands.filter(
          (row) => row.id === "go-date" || (typeof row.label === "string" && row.label.toLowerCase().includes(lower)),
        )
      : commands.filter((row) => ["jump", "view-week", "settings"].includes(row.id));
    out.push(...matching);
    if (create) out.push(create);
    else
      out.push({
        id: "new",
        kind: "New",
        label: "Create task…",
        hint: "+tags @project key:value",
        kbd: "C",
        run: () => undefined,
      });
    return out;
  }, [query, mode, view, running, now, tz, weekStart, actions, navigate]);

  const runRow = (row: Row, alt = false) => {
    if (row.id === "new") return; // needs a title: keep typing
    close();
    (alt && row.alt ? row.alt : row.run)();
  };

  return (
    <Command
      value={selected}
      onValueChange={setSelected}
      label="Commands"
      onKeyDown={(event) => {
        // ⌘⏎ before cmdk's own Enter handling.
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          const row = rows.find((r) => r.id === selected) ?? rows[0];
          if (row) runRow(row, true);
        }
      }}
    >
      <div className="flex h-[46px] items-center gap-[10px] border-b px-[14px] text-[15px]">
        <CaretRight size={14} className="text-faint" />
        <CommandInput
          autoFocus
          value={query}
          onValueChange={setQuery}
          placeholder={mode === "create" ? "New task: title +tag @project key:value" : "Start a task, jump to a date, run a command…"}
        />
        <Kbd>Esc</Kbd>
      </div>
      <CommandList className="p-[6px]">
        {rows.map((row) => (
          <CommandItem key={row.id} value={row.id} onSelect={() => runRow(row)} className="h-9 gap-[10px] px-3">
            <span className="w-[38px] shrink-0 text-[10.5px] uppercase tracking-[.06em] text-faint">{row.kind}</span>
            <span
              className="size-2 shrink-0 rounded-xs"
              style={{ background: row.color ?? "transparent", visibility: row.color ? "visible" : "hidden" }}
            />
            <span className="min-w-0 flex-1 truncate text-[13px]">{row.label}</span>
            {row.hint && <span className="shrink-0 text-[11px] text-faint">{row.hint}</span>}
            <Kbd className="min-w-[18px] text-center" style={{ visibility: row.kbd ? "visible" : "hidden" }}>
              {row.kbd ?? "·"}
            </Kbd>
          </CommandItem>
        ))}
      </CommandList>
      <div className="flex gap-[14px] border-t px-[14px] py-2 text-[11px] text-faint">
        <span>
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd> move
        </span>
        <span>
          <Kbd>⏎</Kbd> run
        </span>
        <span>
          <Kbd>{MOD}⏎</Kbd> start &amp; open
        </span>
        <span className="ml-auto">fuzzy over titles, tags, metadata</span>
      </div>
    </Command>
  );
}
