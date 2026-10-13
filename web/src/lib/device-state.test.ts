import { beforeEach, describe, expect, it } from "vitest";

import { getLastProject, getView, reloadLastProject, reloadViewState, rememberedDate, setLastProject, updateView } from "./device-state.ts";
import { DEFAULT_SETTINGS, getSettings, updateSettings } from "./settings.ts";

beforeEach(() => {
  localStorage.clear();
  reloadViewState();
  reloadLastProject();
});

describe("view store", () => {
  it("round-trips through storage", () => {
    updateView({ range: "week", date: "2026-10-03", savedOn: "2026-10-03", tasksMode: "board" });
    reloadViewState();
    expect(getView()).toEqual({ range: "week", date: "2026-10-03", savedOn: "2026-10-03", tasksMode: "board" });
  });

  it("falls back to defaults on corrupt JSON or values", () => {
    localStorage.setItem("tt.view", "{nope");
    reloadViewState();
    expect(getView().range).toBe("day");
    localStorage.setItem("tt.view", JSON.stringify({ range: "year", date: "x", tasksMode: 3 }));
    reloadViewState();
    expect(getView()).toMatchObject({ range: "day", date: "", tasksMode: "list" });
  });

  it("returns the date only when saved on today", () => {
    updateView({ range: "week", date: "2026-10-03", savedOn: "2026-10-03" });
    expect(rememberedDate(getView(), { year: 2026, month: 10, day: 3 })).toEqual({ year: 2026, month: 10, day: 3 });
    expect(rememberedDate(getView(), { year: 2026, month: 10, day: 4 })).toBeUndefined();
  });
});

describe("last project store", () => {
  it("round-trips and ignores corrupt values", () => {
    setLastProject("p1");
    reloadLastProject();
    expect(getLastProject()).toBe("p1");
    localStorage.setItem("tt.lastProject", "{bad");
    reloadLastProject();
    expect(getLastProject()).toBeNull();
  });
});

describe("stickyProject setting", () => {
  it("defaults to true and survives a corrupt stored value", () => {
    expect(DEFAULT_SETTINGS.stickyProject).toBe(true);
    updateSettings({ stickyProject: false });
    expect(getSettings().stickyProject).toBe(false);
    updateSettings({ stickyProject: true });
  });
});
