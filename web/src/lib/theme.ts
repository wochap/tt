// Applies the theme setting to <html data-theme>. System follows
// `prefers-color-scheme` live, without reload.

import { useEffect } from "react";

import { type ThemeSetting, useSettings } from "./settings.ts";

export type Flavor = "mocha" | "latte";

export function resolveFlavor(theme: ThemeSetting, prefersDark: boolean): Flavor {
  if (theme === "system") return prefersDark ? "mocha" : "latte";
  return theme;
}

export function applyFlavor(flavor: Flavor): void {
  const root = document.documentElement;
  root.dataset.theme = flavor;
  const color = flavor === "mocha" ? "#181825" : "#e6e9ef";
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", color);
}

export function useThemeSync(): void {
  const { theme } = useSettings();
  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => applyFlavor(resolveFlavor(theme, media.matches));
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}
