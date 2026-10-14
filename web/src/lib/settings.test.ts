import { afterEach, describe, expect, it } from "vitest";

import { readSettings } from "./settings.ts";

afterEach(() => localStorage.removeItem("tt.settings"));

describe("settings grids", () => {
  it("default to 15/15", () => {
    localStorage.removeItem("tt.settings");
    expect(readSettings()).toMatchObject({ startGrid: 15, durationGrid: 15 });
  });

  it("migrate a stored snap of 10 to 10/10", () => {
    localStorage.setItem("tt.settings", JSON.stringify({ snapMinutes: 10 }));
    const settings = readSettings();
    expect(settings).toMatchObject({ startGrid: 10, durationGrid: 10 });
    expect(settings).not.toHaveProperty("snapMinutes");
  });

  it("migrate a stored snap of off to off/15", () => {
    localStorage.setItem("tt.settings", JSON.stringify({ snapMinutes: 0 }));
    expect(readSettings()).toMatchObject({ startGrid: 0, durationGrid: 15 });
  });
});
