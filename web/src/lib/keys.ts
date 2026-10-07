// Keyboard model: one key, one verb. Bindings are strings like "mod+k",
// "shift+s", "?", "j", "alt+1", "enter", "backspace"; "|" separates
// alternatives ("j|arrowdown"). Layers form a stack: the most recently
// mounted layer (an open sheet, a popover) sees a key first, the global layer
// mounted at startup last. A handler returning `false` passes the key on.

import { useEffect, useRef } from "react";

import { isMac } from "./utils.ts";

export type KeyHandler = (event: KeyboardEvent) => void | boolean;

export interface KeyOptions {
  /** Also fire while focus is in a text field (default only for mod/Escape bindings). */
  inInputs?: boolean;
  enabled?: boolean;
}

export function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag !== "INPUT") return false;
  const type = (target as HTMLInputElement).type;
  return !["checkbox", "radio", "button", "submit", "range"].includes(type);
}

function normalizeKey(key: string): string {
  const k = key.toLowerCase();
  return k === " " ? "space" : k === "esc" ? "escape" : k;
}

export function matches(binding: string, event: KeyboardEvent): boolean {
  const parts = binding.toLowerCase().split("+");
  const key = parts.pop()!;
  const want = new Set(parts);
  const mod = isMac ? event.metaKey : event.ctrlKey;
  if (want.has("mod") !== mod) return false;
  // The other platform modifier (Ctrl on macOS, Meta elsewhere) never matches.
  if (isMac ? event.ctrlKey : event.metaKey) return false;
  if (want.has("alt") !== event.altKey) return false;
  const eventKey = normalizeKey(event.key);
  // Shifted punctuation ("?", "{") is matched by the produced character.
  const isSymbol = key.length === 1 && !/[a-z0-9]/.test(key);
  if (!isSymbol && want.has("shift") !== event.shiftKey) return false;
  if (key === eventKey) return true;
  // Alt changes the produced character on macOS; fall back to the physical key.
  if (event.altKey && /^[a-z0-9]$/.test(key)) return event.code === (/\d/.test(key) ? `Digit${key}` : `Key${key.toUpperCase()}`);
  return false;
}

interface Layer {
  bindings: { current: Record<string, KeyHandler> };
  inInputs: boolean;
}

const layers: Layer[] = [];

function dispatch(event: KeyboardEvent): void {
  if (event.defaultPrevented || event.isComposing) return;
  const editable = isEditable(event.target);
  for (let i = layers.length - 1; i >= 0; i--) {
    const layer = layers[i]!;
    for (const [binding, handler] of Object.entries(layer.bindings.current)) {
      const modBinding = /(^|\+)mod\+/.test(binding) || binding === "escape";
      if (editable && !layer.inInputs && !modBinding) continue;
      for (const alternative of binding.split("|")) {
        if (!matches(alternative.trim(), event)) continue;
        if (handler(event) === false) break;
        event.preventDefault();
        return;
      }
    }
  }
}

if (typeof window !== "undefined") window.addEventListener("keydown", dispatch);

/** Registers a layer of bindings for the lifetime of the component. */
export function useKeys(bindings: Record<string, KeyHandler>, options: KeyOptions = {}): void {
  const ref = useRef(bindings);
  ref.current = bindings;
  const { enabled = true, inInputs = false } = options;
  useEffect(() => {
    if (!enabled) return;
    const layer: Layer = { bindings: ref, inInputs };
    layers.push(layer);
    return () => {
      const at = layers.indexOf(layer);
      if (at >= 0) layers.splice(at, 1);
    };
  }, [enabled, inInputs]);
}
