import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Kbd } from "@/components/ui/kbd";

import { SHORTCUTS } from "./shortcuts.ts";
import { useUi } from "./ui-state.tsx";

/** `?`: every shortcut grouped by scope (the only dialog besides logout). */
export function ShortcutSheet() {
  const { shortcutsOpen, setShortcutsOpen } = useUi();
  return (
    <Dialog open={shortcutsOpen} onOpenChange={setShortcutsOpen}>
      <DialogContent className="top-[28px]">
        <div className="flex items-center justify-between gap-4 border-b px-4 py-3">
          <DialogTitle className="text-[15px] font-medium">Keyboard shortcuts</DialogTitle>
          <DialogDescription className="text-right text-[11.5px] text-muted">
            One key, one verb: <Kbd>S</Kbd> always toggles tracking for the focused thing · <Kbd>J</Kbd>
            <Kbd>K</Kbd> always mean next / previous · <Kbd>Esc</Kbd>
          </DialogDescription>
        </div>
        <div className="grid grid-cols-1 gap-x-5 gap-y-3 px-4 py-[14px] sm:grid-cols-2">
          {SHORTCUTS.map((group) => (
            <section key={group.scope} className="flex flex-col gap-[3px]" aria-label={group.scope}>
              <h3 className="mb-[3px] text-[11px] uppercase tracking-[.08em] text-link">{group.scope}</h3>
              {group.keys.map(([key, verb]) => (
                <div key={key + verb} className="grid min-h-[22px] grid-cols-[96px_1fr] items-center gap-[10px] text-[12px]">
                  <span className="font-mono text-[11px] text-fg">{key}</span>
                  <span className="text-muted">{verb}</span>
                </div>
              ))}
            </section>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
