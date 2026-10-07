// Service worker registration (vite-plugin-pwa) and the install prompt.

import { useSyncExternalStore } from "react";

interface InstallPromptEvent extends Event {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

let deferred: InstallPromptEvent | null = null;
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((listener) => listener());

export function initPwa(): void {
  addEventListener("beforeinstallprompt", (event) => {
    event.preventDefault();
    deferred = event as InstallPromptEvent;
    notify();
  });
  addEventListener("appinstalled", () => {
    deferred = null;
    notify();
  });
  if (import.meta.env.PROD && "serviceWorker" in navigator) {
    void import("virtual:pwa-register").then(({ registerSW }) => registerSW({ immediate: true }));
  }
}

export function useInstallPrompt(): (() => Promise<void>) | null {
  const available = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => deferred !== null,
  );
  if (!available) return null;
  return async () => {
    const event = deferred;
    if (!event) return;
    await event.prompt();
    await event.userChoice;
    deferred = null;
    notify();
  };
}

/**
 * Removes cached user data (runtime caches such as API responses). The
 * precached app shell holds no user data and stays, so the login page still
 * loads after an offline logout.
 */
export async function clearCaches(): Promise<void> {
  if (typeof caches === "undefined") return;
  const keys = await caches.keys();
  await Promise.all(keys.filter((key) => !key.startsWith("workbox-precache")).map((key) => caches.delete(key)));
}
