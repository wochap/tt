import "@testing-library/jest-dom/vitest";

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

import { initializeWasm, isWasmInitialized } from "@automerge/automerge/slim";

// The app loads the wasm by URL (src/sync/wasm.ts); tests read it from disk.
if (!isWasmInitialized()) {
  const require = createRequire(import.meta.url);
  const path = require.resolve("@automerge/automerge/automerge.wasm");
  await initializeWasm(readFileSync(path));
}

// jsdom lacks these.
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList;
}
if (!("ResizeObserver" in window)) {
  (window as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}
Element.prototype.scrollIntoView ??= function () {};
