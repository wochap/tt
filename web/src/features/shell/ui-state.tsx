// App-wide UI state that is not in the URL: palette and shortcut sheet, plus
// status-bar hints contributed by the current view.

import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";

export type PaletteMode = "all" | "create";

interface UiState {
  palette: { open: boolean; mode: PaletteMode; query: string };
  openPalette: (mode?: PaletteMode, query?: string) => void;
  closePalette: () => void;
  shortcutsOpen: boolean;
  setShortcutsOpen: (open: boolean) => void;
  hints: ReactNode;
  setHints: (hints: ReactNode) => void;
}

const UiContext = createContext<UiState | null>(null);

export function UiProvider({ children }: { children: ReactNode }) {
  const [palette, setPalette] = useState<UiState["palette"]>({ open: false, mode: "all", query: "" });
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [hints, setHints] = useState<ReactNode>(null);
  const value = useMemo<UiState>(
    () => ({
      palette,
      openPalette: (mode = "all", query = "") => setPalette({ open: true, mode, query }),
      closePalette: () => setPalette((p) => ({ ...p, open: false })),
      shortcutsOpen,
      setShortcutsOpen,
      hints,
      setHints,
    }),
    [palette, shortcutsOpen, hints],
  );
  return <UiContext.Provider value={value}>{children}</UiContext.Provider>;
}

export function useUi(): UiState {
  const ui = useContext(UiContext);
  if (!ui) throw new Error("useUi outside UiProvider");
  return ui;
}

/** Contributes key hints to the status bar while the calling view is mounted. */
export function useStatusHints(hints: ReactNode, deps: unknown[]): void {
  const { setHints } = useUi();
  useEffect(() => {
    setHints(hints);
    return () => setHints(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
