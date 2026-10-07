// Automerge's wasm, loaded once per JS context (page and worker). Every
// Automerge import resolves to the slim build (see vite.config.ts), so this
// must settle before any document is touched.

import { initializeWasm, isWasmInitialized } from "@automerge/automerge/slim";
import wasmUrl from "@automerge/automerge/automerge.wasm?url";

let ready: Promise<void> | undefined;

export function loadAutomerge(): Promise<void> {
  if (isWasmInitialized()) return Promise.resolve();
  ready ??= initializeWasm(wasmUrl);
  return ready;
}
