import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useEdgeScroll } from "./useEdgeScroll.ts";

describe("useEdgeScroll", () => {
  let frames: Map<number, FrameRequestCallback>;
  let nextId: number;
  const flush = () => {
    const pending = [...frames.entries()];
    frames.clear();
    for (const [, cb] of pending) cb(0);
  };

  beforeEach(() => {
    frames = new Map();
    nextId = 1;
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      frames.set(nextId, cb);
      return nextId++;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => void frames.delete(id));
  });
  afterEach(() => vi.unstubAllGlobals());

  const scroller = () => {
    const el = document.createElement("div");
    el.getBoundingClientRect = () => new DOMRect(0, 0, 300, 1000);
    Object.defineProperty(el, "scrollTop", { value: 500, writable: true });
    return el;
  };

  it("scrolls near the edge and reports it, then stops once inactive", () => {
    const el = scroller();
    const onScroll = vi.fn();
    const { rerender } = renderHook(({ active }) => useEdgeScroll(active, { current: el }, () => 995, onScroll), { initialProps: { active: true } });
    flush();
    expect(el.scrollTop).toBeGreaterThan(500);
    expect(onScroll).toHaveBeenCalledTimes(1);
    rerender({ active: false });
    expect(frames.size).toBe(0);
    const at = el.scrollTop;
    flush();
    expect(el.scrollTop).toBe(at);
  });

  it("does not scroll in the middle and cancels on unmount", () => {
    const el = scroller();
    const onScroll = vi.fn();
    const { unmount } = renderHook(() => useEdgeScroll(true, { current: el }, () => 500, onScroll));
    flush();
    expect(el.scrollTop).toBe(500);
    expect(onScroll).not.toHaveBeenCalled();
    unmount();
    expect(frames.size).toBe(0);
  });
});
