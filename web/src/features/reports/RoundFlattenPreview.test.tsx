import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { type RoundOptions, roundAndFlatten, type View } from "@tt/domain";

import { RoundFlattenPreview } from "./RoundFlattenPreview.tsx";

const tz = "UTC";
const t = (d: number, h: number, m: number) => Date.UTC(2026, 9, d, h, m);
const view = { workspace: { tasks: new Map(), projects: new Map(), tags: new Map() }, entries: new Map() } as unknown as View;
const options: RoundOptions = { gridSeconds: 900, mode: "up", startGridSeconds: 0, startMode: "up", group: "entry" };

function renderPreview(inputs: { task: string; start: number; end: number; entries: string[] }[], range: { from: number; to: number }) {
  const rounded = roundAndFlatten(inputs, options, tz);
  render(<RoundFlattenPreview view={view} tz={tz} inputs={inputs} rounded={rounded} group="entry" gridMinutes={15} mode="up" startGridMinutes={0} weekStart={0} range={range} />);
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("RoundFlattenPreview", () => {
  it("hovering a raw half highlights its rounded half and reveals the raw time", () => {
    renderPreview([{ task: "a", start: t(7, 13, 10), end: t(7, 13, 45), entries: ["e1"] }], { from: t(7, 0, 0), to: t(8, 0, 0) });
    expect(screen.queryByTestId("day-header")).toBeNull();
    const pair = screen.getByTestId("pair-row");
    expect(screen.queryByTestId("raw-reveal")).toBeNull();
    fireEvent.mouseEnter(within(pair).getByTestId("raw-row"));
    expect(pair).toHaveAttribute("data-hovered");
    expect(within(within(pair).getByTestId("flat-row")).getByTestId("raw-reveal")).toHaveTextContent("13:10–13:45");
    fireEvent.mouseLeave(within(pair).getByTestId("raw-row"));
    expect(pair).not.toHaveAttribute("data-hovered");
  });

  it("drives the day strip from the pinned header and from header clicks", () => {
    let callback: IntersectionObserverCallback = () => {};
    const observed: Element[] = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(cb: IntersectionObserverCallback) {
          callback = cb;
        }
        observe(el: Element) {
          observed.push(el);
        }
        disconnect() {}
      },
    );
    renderPreview(
      [
        { task: "a", start: t(5, 9, 0), end: t(5, 10, 0), entries: ["mon"] },
        { task: "a", start: t(6, 9, 0), end: t(6, 10, 0), entries: ["tue"] },
        { task: "a", start: t(8, 9, 0), end: t(8, 10, 0), entries: ["thu"] },
      ],
      { from: t(5, 0, 0), to: t(12, 0, 0) },
    );
    expect(screen.getAllByTestId("day-header")).toHaveLength(3);
    expect(screen.getByTestId("strip-day")).toHaveTextContent("Mon 5 Oct");
    const tue = observed.find((el) => (el as HTMLElement).dataset.day === "2026-10-06")!;
    act(() => callback([{ target: tue, isIntersecting: true } as unknown as IntersectionObserverEntry], {} as IntersectionObserver));
    expect(screen.getByTestId("strip-day")).toHaveTextContent("Tue 6 Oct");
    fireEvent.click(screen.getAllByTestId("day-header")[2]!);
    expect(screen.getByTestId("strip-day")).toHaveTextContent("Thu 8 Oct");
    fireEvent.click(screen.getByRole("button", { name: "Previous day" }));
    expect(screen.getByTestId("strip-day")).toHaveTextContent("Tue 6 Oct");
  });
});
