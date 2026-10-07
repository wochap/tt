// Quick-create syntax `title +tag @project key:value`, mirroring `parse_quick`
// in `tt-core/src/text.rs`. A single input line is tokenized shell-style
// (quoted runs stay one argument) before the same argument rules apply.

import type { NewTask } from "./ops.ts";

/** `key:value` or `key=value` with a key that starts with a letter. */
export function metadataPair(arg: string): [string, string] | undefined {
  const colon = arg.indexOf(":");
  const equals = arg.indexOf("=");
  const at = colon >= 0 ? colon : equals;
  if (at < 0) return undefined;
  const key = arg.slice(0, at);
  const value = arg.slice(at + 1);
  const validKey = /^[A-Za-z][A-Za-z0-9_.-]*$/.test(key);
  return validKey && value !== "" && !value.startsWith("//") ? [key, value] : undefined;
}

/** Parses already-split arguments exactly like `tt task add`. */
export function parseQuickArgs(args: readonly string[]): NewTask & { tags: string[]; metadata: Record<string, string> } {
  const task: NewTask & { tags: string[]; metadata: Record<string, string> } = { title: "", tags: [], metadata: {} };
  const title: string[] = [];
  for (const arg of args) {
    if (/\s/.test(arg)) title.push(arg);
    else if (arg.startsWith("+") && arg.length > 1) task.tags.push(arg.slice(1));
    else if (arg.startsWith("@") && arg.length > 1) task.project = arg.slice(1);
    else {
      const pair = metadataPair(arg);
      if (pair) task.metadata[pair[0]] = pair[1];
      else title.push(arg);
    }
  }
  task.title = title.join(" ");
  return task;
}

export type QuickTokenKind = "text" | "tag" | "project" | "meta" | "space";

export interface QuickToken {
  kind: QuickTokenKind;
  /** Source text including any quotes. */
  text: string;
  /** Argument value (quotes removed). */
  value: string;
}

/** Splits a line into tokens, keeping whitespace so the input can be redrawn. */
export function tokenizeQuick(line: string): QuickToken[] {
  const tokens: QuickToken[] = [];
  let i = 0;
  while (i < line.length) {
    const ch = line[i]!;
    if (/\s/.test(ch)) {
      let j = i;
      while (j < line.length && /\s/.test(line[j]!)) j++;
      tokens.push({ kind: "space", text: line.slice(i, j), value: line.slice(i, j) });
      i = j;
      continue;
    }
    if (ch === '"' || ch === "'") {
      const close = line.indexOf(ch, i + 1);
      const j = close < 0 ? line.length : close + 1;
      const value = line.slice(i + 1, close < 0 ? line.length : close);
      tokens.push({ kind: "text", text: line.slice(i, j), value });
      i = j;
      continue;
    }
    let j = i;
    while (j < line.length && !/\s/.test(line[j]!)) j++;
    const word = line.slice(i, j);
    let kind: QuickTokenKind = "text";
    if (word.startsWith("+") && word.length > 1) kind = "tag";
    else if (word.startsWith("@") && word.length > 1) kind = "project";
    else if (metadataPair(word)) kind = "meta";
    tokens.push({ kind, text: word, value: word });
    i = j;
  }
  return tokens;
}

/** Parses a quick-create line. */
export function parseQuick(line: string): NewTask & { tags: string[]; metadata: Record<string, string> } {
  const args = tokenizeQuick(line)
    .filter((token) => token.kind !== "space")
    .map((token) => token.value);
  return parseQuickArgs(args);
}
