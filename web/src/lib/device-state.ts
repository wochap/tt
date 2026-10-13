// Device-local UI state (not synced): the last timeline/tasks view and the last
// project a task was created with. Separate keys from `tt.settings`, so
// navigating never rewrites the user's preferences. A missing or corrupt value
// means the default behavior.

import { useSyncExternalStore } from "react";

import { type CivilDate, formatDate, parseDate } from "@tt/domain";

export type RangeMode = "day" | "week" | "month";
export type TasksMode = "list" | "board";

export interface ViewState {
  range: RangeMode;
  /** Civil date `YYYY-MM-DD` shown when it was saved. */
  date: string;
  /** Civil date the state was saved on, in the app's time zone. */
  savedOn: string;
  tasksMode: TasksMode;
}

export const DEFAULT_VIEW: ViewState = { range: "day", date: "", savedOn: "", tasksMode: "list" };

const VIEW_KEY = "tt.view";
const PROJECT_KEY = "tt.lastProject";

function store<T>(key: string, parse: (raw: unknown) => T) {
  const listeners = new Set<() => void>();
  const read = (): T => {
    try {
      return parse(JSON.parse(localStorage.getItem(key) ?? "null"));
    } catch {
      return parse(null);
    }
  };
  let current = read();
  if (typeof addEventListener !== "undefined") {
    addEventListener("storage", (event) => {
      if (event.key === key) {
        current = read();
        for (const listener of listeners) listener();
      }
    });
  }
  const subscribe = (listener: () => void) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  return {
    get: () => current,
    set(next: T) {
      current = next;
      try {
        localStorage.setItem(key, JSON.stringify(next));
      } catch {
        // storage full or blocked: keep the in-memory value
      }
      for (const listener of listeners) listener();
    },
    /** Drop the cache and re-read storage (tests, manual edits). */
    reload() {
      current = read();
      for (const listener of listeners) listener();
    },
    use: () => useSyncExternalStore(subscribe, () => current, () => current),
  };
}

const isDate = (value: unknown): value is string => typeof value === "string" && parseDate(value) !== undefined;

function parseView(raw: unknown): ViewState {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  return {
    range: r.range === "week" || r.range === "month" ? r.range : "day",
    date: isDate(r.date) ? r.date : "",
    savedOn: isDate(r.savedOn) ? r.savedOn : "",
    tasksMode: r.tasksMode === "board" ? "board" : "list",
  };
}

const view = store<ViewState>(VIEW_KEY, parseView);
const project = store<string | null>(PROJECT_KEY, (raw) => (typeof raw === "string" && raw ? raw : null));

export const getView = view.get;
export const useViewState = view.use;
export const reloadViewState = view.reload;

export function updateView(patch: Partial<ViewState>): void {
  const next = { ...view.get(), ...patch };
  const cur = view.get();
  if (next.range === cur.range && next.date === cur.date && next.savedOn === cur.savedOn && next.tasksMode === cur.tasksMode) return;
  view.set(next);
}

/** The remembered date, only when it was saved on `today` (same calendar day). */
export function rememberedDate(state: ViewState, today: CivilDate): CivilDate | undefined {
  if (!state.date || state.savedOn !== formatDate(today)) return undefined;
  return parseDate(state.date);
}

export const getLastProject = project.get;
export const useLastProject = project.use;
export const reloadLastProject = project.reload;
export const setLastProject = (id: string): void => {
  if (project.get() !== id) project.set(id);
};
