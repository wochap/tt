import { useSyncExternalStore } from "react";

export function useMedia(query: string): boolean {
  return useSyncExternalStore(
    (listener) => {
      const media = matchMedia(query);
      media.addEventListener("change", listener);
      return () => media.removeEventListener("change", listener);
    },
    () => matchMedia(query).matches,
    () => false,
  );
}

/** Phone-width layout (the 390 px frames). */
export function usePhone(): boolean {
  return useMedia("(max-width: 640px)");
}
