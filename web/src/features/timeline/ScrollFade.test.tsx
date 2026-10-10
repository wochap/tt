import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { ScrollFade, useMoreBelow } from "./ScrollFade.tsx";

function Scroller() {
  const ref = useRef<HTMLDivElement>(null);
  const more = useMoreBelow(ref);
  return (
    <div ref={ref} data-testid="scroller">
      <div style={{ height: 1000 }} />
      <ScrollFade show={more} />
    </div>
  );
}

/** jsdom has no layout: give the scroller its metrics. */
function metrics(element: HTMLElement, m: { scrollTop: number; clientHeight: number; scrollHeight: number }) {
  for (const [key, value] of Object.entries(m)) Object.defineProperty(element, key, { value, configurable: true });
}

describe("phone day scroll affordance", () => {
  afterEach(cleanup);

  it("shows the fade and indicator while hours are below, hides them at the end", () => {
    const { rerender } = render(<Scroller />);
    const scroller = screen.getByTestId("scroller");
    metrics(scroller, { scrollTop: 0, clientHeight: 400, scrollHeight: 1000 });
    act(() => {
      fireEvent.scroll(scroller);
    });
    rerender(<Scroller />);
    expect(screen.getByTestId("scroll-fade")).toBeInTheDocument();
    expect(screen.getByTestId("scroll-indicator")).toBeInTheDocument();
    metrics(scroller, { scrollTop: 600, clientHeight: 400, scrollHeight: 1000 });
    act(() => {
      fireEvent.scroll(scroller);
    });
    expect(screen.queryByTestId("scroll-fade")).toBeNull();
    metrics(scroller, { scrollTop: 300, clientHeight: 400, scrollHeight: 1000 });
    act(() => {
      fireEvent.scroll(scroller);
    });
    expect(screen.getByTestId("scroll-fade")).toBeInTheDocument();
  });
});
