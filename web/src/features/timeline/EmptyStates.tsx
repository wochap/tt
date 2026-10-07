import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";

import { useUi } from "../shell/ui-state.tsx";

// Frame 14: what a brand-new user sees. Overlays sit on the empty grid and
// leave it interactive (pointer events pass through except on the actions).

export function EmptyDay({ hasTasks, onStart }: { hasTasks: boolean; onStart: () => void }) {
  const { openPalette } = useUi();
  return (
    <div className="pointer-events-none absolute left-[60px] right-[30px] top-[70px] z-10 flex flex-col items-start gap-[10px]" data-testid="empty-day">
      <div className="flex h-[46px] w-full items-center rounded-2 border border-dashed border-s2 bg-base px-[10px] text-[12px] text-muted">
        Drag anywhere on the grid to add your first entry
      </div>
      <div className="text-[12px] leading-[1.5] text-muted">Or start a timer and come back later — entries grow to “now”.</div>
      <div className="pointer-events-auto flex gap-2">
        <Button variant="primary" className="gap-2" onClick={onStart}>
          Start tracking <Kbd inherit>S</Kbd>
        </Button>
        {!hasTasks && (
          <Button variant="secondary" onClick={() => openPalette("create")}>
            Create your first task
          </Button>
        )}
      </div>
    </div>
  );
}

export function EmptyWeek() {
  const { openPalette } = useUi();
  return (
    <div
      className="pointer-events-auto absolute left-[92px] right-10 top-[100px] z-10 flex max-w-[460px] flex-col items-start gap-2 rounded-md bg-mantle p-[14px] shadow-[var(--shadow-md)]"
      data-testid="empty-week"
    >
      <div className="text-[13px] font-medium">Your week is empty</div>
      <div className="text-[12px] leading-[1.5] text-muted">
        Track from here, or from the CLI: <span className="font-mono text-fg">tt start 1</span>. Entries sync into this grid.
      </div>
      <Button variant="primary" size="sm" onClick={() => openPalette("create")}>
        Create first task
      </Button>
    </div>
  );
}

export function EmptyMonth({ onToday }: { onToday: () => void }) {
  return (
    <div
      className="absolute left-1/2 top-[30%] z-10 flex w-[min(420px,80%)] -translate-x-1/2 flex-col items-start gap-2 rounded-md bg-mantle p-[14px] shadow-[var(--shadow-md)]"
      data-testid="empty-month"
    >
      <div className="text-[13px] font-medium">No time tracked this month</div>
      <div className="text-[12px] leading-[1.5] text-muted">Days fill with totals and your top tasks as you track.</div>
      <button type="button" onClick={onToday} className="flex items-center gap-[6px] text-[12.5px] text-link hover:underline">
        Open today <Kbd>T</Kbd>
      </button>
    </div>
  );
}
