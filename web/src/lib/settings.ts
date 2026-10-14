// Device-local preferences (not synced): theme, week start, start and
// duration grids, time
// zone, visible hours. Stored in localStorage; every tab follows changes.

import { useSyncExternalStore } from "react";

import { resolveTz, systemTimeZone, type Weekday } from "@tt/domain";

import type { DurationGrid, StartGrid } from "./grids.ts";

export type ThemeSetting = "system" | "latte" | "mocha";

export interface Settings {
  theme: ThemeSetting;
  /** Monday = 0 … Sunday = 6. */
  weekStart: Weekday;
  /** Default start grid in minutes (0 = off) for the timeline and Reports. */
  startGrid: StartGrid;
  /** Default duration grid in minutes for the timeline and Reports. */
  durationGrid: DurationGrid;
  /** IANA zone, or "" to follow the system. */
  timeZone: string;
  /** First and last visible hour of the timeline (entries outside still render). */
  visibleHours: [number, number];
  /** Month view: how many top tasks a day cell lists. */
  monthTopTasks: number;
  /** Quick create starts with the project of the last created task. */
  stickyProject: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  weekStart: 0,
  startGrid: 15,
  durationGrid: 15,
  timeZone: "",
  visibleHours: [7, 19],
  monthTopTasks: 3,
  stickyProject: true,
};

const KEY = "tt.settings";
const listeners = new Set<() => void>();
let current: Settings = readSettings();

export function readSettings(): Settings {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Settings> & { snapMinutes?: number };
    const { snapMinutes, ...rest } = raw;
    if (typeof snapMinutes === "number" && rest.startGrid === undefined && rest.durationGrid === undefined) {
      // Single snap value from before the split: start = snap, duration = snap || 15.
      rest.startGrid = snapMinutes as StartGrid;
      rest.durationGrid = (snapMinutes || 15) as DurationGrid;
    }
    const merged = { ...DEFAULT_SETTINGS, ...rest };
    if (typeof merged.stickyProject !== "boolean") merged.stickyProject = true;
    return merged;
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

if (typeof addEventListener !== "undefined") {
  addEventListener("storage", (event) => {
    if (event.key === KEY) {
      current = readSettings();
      for (const listener of listeners) listener();
    }
  });
}

export function getSettings(): Settings {
  return current;
}

export function updateSettings(patch: Partial<Settings>): void {
  current = { ...current, ...patch };
  localStorage.setItem(KEY, JSON.stringify(current));
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useSettings(): Settings {
  return useSyncExternalStore(subscribe, getSettings, getSettings);
}

/** Effective IANA zone of the settings. */
export function effectiveTz(settings: Settings): string {
  return resolveTz(settings.timeZone || undefined, systemTimeZone());
}

export function useTz(): string {
  return effectiveTz(useSettings());
}
