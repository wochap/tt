// Device-local preferences (not synced): theme, week start, snap grid, time
// zone, visible hours. Stored in localStorage; every tab follows changes.

import { useSyncExternalStore } from "react";

import { resolveTz, systemTimeZone, type Weekday } from "@tt/domain";

export type ThemeSetting = "system" | "latte" | "mocha";
export type SnapMinutes = 0 | 5 | 10 | 15;

export interface Settings {
  theme: ThemeSetting;
  /** Monday = 0 … Sunday = 6. */
  weekStart: Weekday;
  snapMinutes: SnapMinutes;
  /** IANA zone, or "" to follow the system. */
  timeZone: string;
  /** First and last visible hour of the timeline (entries outside still render). */
  visibleHours: [number, number];
  /** Month view: how many top tasks a day cell lists. */
  monthTopTasks: number;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  weekStart: 0,
  snapMinutes: 15,
  timeZone: "",
  visibleHours: [7, 19],
  monthTopTasks: 3,
};

const KEY = "tt.settings";
const listeners = new Set<() => void>();
let current: Settings = read();

function read(): Settings {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Settings>;
    return { ...DEFAULT_SETTINGS, ...raw };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

if (typeof addEventListener !== "undefined") {
  addEventListener("storage", (event) => {
    if (event.key === KEY) {
      current = read();
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
