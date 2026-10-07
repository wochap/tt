import { describe, expect, it } from "vitest";

import { matches } from "./keys.ts";

const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

describe("key bindings", () => {
  it("matches plain, shifted and modified keys", () => {
    expect(matches("j", key({ key: "j" }))).toBe(true);
    expect(matches("j", key({ key: "J", shiftKey: true }))).toBe(false);
    expect(matches("shift+s", key({ key: "S", shiftKey: true }))).toBe(true);
    expect(matches("?", key({ key: "?", shiftKey: true }))).toBe(true);
    expect(matches("mod+k", key({ key: "k", ctrlKey: true }))).toBe(true);
    expect(matches("mod+z", key({ key: "Z", ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(matches("shift+mod+z", key({ key: "Z", ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(matches("alt+1", key({ key: "¡", code: "Digit1", altKey: true }))).toBe(true);
  });
});
