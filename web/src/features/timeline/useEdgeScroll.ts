import { type RefObject, useEffect, useRef } from "react";

import { edgeScrollSpeed } from "./drag.ts";

/**
 * While `active`, scrolls `scroller` when the pointer is in its top/bottom edge zone
 * and calls `onScroll` after each step so the caller can recompute the preview.
 * The frame loop runs only while active and is cancelled when it ends or on unmount.
 */
export function useEdgeScroll(active: boolean, scroller: RefObject<HTMLElement | null>, pointerY: () => number, onScroll: () => void) {
  const latest = useRef({ pointerY, onScroll });
  latest.current = { pointerY, onScroll };
  useEffect(() => {
    if (!active) return;
    let frame = requestAnimationFrame(tick);
    function tick() {
      const el = scroller.current;
      if (el) {
        const speed = edgeScrollSpeed(latest.current.pointerY(), el.getBoundingClientRect());
        if (speed) {
          const before = el.scrollTop;
          el.scrollTop = before + speed;
          if (el.scrollTop !== before) latest.current.onScroll();
        }
      }
      frame = requestAnimationFrame(tick);
    }
    return () => cancelAnimationFrame(frame);
  }, [active, scroller]);
}
