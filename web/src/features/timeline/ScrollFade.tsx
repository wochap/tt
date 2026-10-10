import { CaretDown } from "@phosphor-icons/react";
import { type RefObject, useEffect, useState } from "react";

/** True while `scroller` has content below its visible area. */
export function useMoreBelow(scroller: RefObject<HTMLElement | null>, enabled = true): boolean {
  const [more, setMore] = useState(false);
  useEffect(() => {
    const element = scroller.current;
    if (!enabled || !element) {
      setMore(false);
      return;
    }
    const update = () => setMore(element.scrollTop + element.clientHeight < element.scrollHeight - 1);
    update();
    element.addEventListener("scroll", update, { passive: true });
    const observer = new ResizeObserver(update);
    observer.observe(element);
    if (element.firstElementChild) observer.observe(element.firstElementChild);
    return () => {
      element.removeEventListener("scroll", update);
      observer.disconnect();
    };
  }, [scroller, enabled]);
  return more;
}

/**
 * Phone day (Turn 6): a fade and a scroll indicator at the bottom edge of the
 * scrolling timeline while more hours are below. Sticky inside the scroller.
 */
export function ScrollFade({ show }: { show: boolean }) {
  if (!show) return null;
  return (
    <div className="pointer-events-none sticky bottom-0 z-20 -mt-12 flex h-12 items-end justify-center bg-gradient-to-t from-base to-transparent pb-1" data-testid="scroll-fade" aria-hidden>
      <span className="flex size-5 items-center justify-center rounded-full bg-s0 text-sub1 shadow-[var(--shadow-sm)]" data-testid="scroll-indicator">
        <CaretDown size={11} weight="bold" />
      </span>
    </div>
  );
}
