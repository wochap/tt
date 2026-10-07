// Fuzzy task search over title, `#seq`, tag names, metadata values and the
// project name (the haystacks of `tt-core/src/search.rs`). Every whitespace
// separated atom of the query must match some field; exact (normalized)
// substring hits on ids such as ticket numbers outrank loose subsequences.

import { compare, type Task, type View } from "./model.ts";

export type FieldKind = "title" | "seq" | "tag" | "meta" | "project";

export interface FieldMatch {
  kind: FieldKind;
  /** Metadata key, tag id or project id. */
  key?: string;
  text: string;
  /** Matched character positions in `text`. */
  indices: number[];
}

export interface Hit {
  task: Task;
  score: number;
  matches: FieldMatch[];
}

interface Field {
  kind: FieldKind;
  key?: string;
  text: string;
}

export function haystacks(view: View, task: Task): Field[] {
  const fields: Field[] = [
    { kind: "title", text: task.title },
    { kind: "seq", text: `#${task.seq}` },
  ];
  for (const id of task.tags) {
    const tag = view.workspace.tags.get(id);
    if (tag) fields.push({ kind: "tag", key: id, text: tag.name });
  }
  for (const [key, value] of Object.entries(task.metadata)) fields.push({ kind: "meta", key, text: value });
  const project = task.project ? view.workspace.projects.get(task.project) : undefined;
  if (project) fields.push({ kind: "project", key: project.id, text: project.name });
  return fields;
}

const isWordChar = (ch: string) => /[\p{L}\p{N}]/u.test(ch);

/**
 * Scores `pattern` (already lower-cased) as a subsequence of `text`.
 * Returns undefined when it does not match.
 */
export function fuzzyScore(pattern: string, text: string): { score: number; indices: number[] } | undefined {
  if (!pattern) return { score: 0, indices: [] };
  const lower = text.toLowerCase();
  // Exact substring on the normalized (alphanumeric-only) forms: ids like PROJ-123 vs "proj123".
  const exact = normalizedSubstring(pattern, text);
  if (exact) return exact;
  let best: { score: number; indices: number[] } | undefined;
  // Try every start position of the first pattern char, keep the best greedy alignment.
  for (let start = lower.indexOf(pattern[0]!); start >= 0; start = lower.indexOf(pattern[0]!, start + 1)) {
    const indices: number[] = [];
    let p = 0;
    for (let i = start; i < lower.length && p < pattern.length; i++) {
      if (lower[i] === pattern[p]) {
        indices.push(i);
        p++;
      }
    }
    if (p < pattern.length) break;
    let score = 0;
    for (let k = 0; k < indices.length; k++) {
      const i = indices[k]!;
      score += 16;
      if (i === 0 || !isWordChar(text[i - 1]!)) score += 8;
      if (k > 0 && indices[k - 1] === i - 1) score += 12;
      else if (k > 0) score -= Math.min(10, i - indices[k - 1]! - 1);
    }
    score -= Math.min(6, indices[0]!);
    if (!best || score > best.score) best = { score, indices };
  }
  return best;
}

function normalizedSubstring(pattern: string, text: string): { score: number; indices: number[] } | undefined {
  const plain = [...pattern].filter(isWordChar).join("");
  if (plain.length < 2) {
    const at = text.toLowerCase().indexOf(pattern);
    return at >= 0 && pattern.length > 0
      ? { score: 30 * pattern.length + (at === 0 ? 20 : 0), indices: [...Array(pattern.length).keys()].map((k) => at + k) }
      : undefined;
  }
  const positions: number[] = [];
  let normalized = "";
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]!;
    if (isWordChar(ch)) {
      normalized += ch.toLowerCase();
      positions.push(i);
    }
  }
  const at = normalized.indexOf(plain);
  if (at < 0) return undefined;
  const first = positions[at]!;
  const last = positions[at + plain.length - 1]!;
  const indices = [];
  for (let i = first; i <= last; i++) indices.push(i);
  const whole = normalized.length === plain.length;
  return { score: 30 * plain.length + (at === 0 ? 20 : 0) + (whole ? 40 : 0), indices };
}

/**
 * Tasks matching `query` ordered by best score, ties broken by lowest seq.
 * An empty query returns every candidate in seq order.
 */
export function search(view: View, query: string, filter: (task: Task) => boolean = () => true): Hit[] {
  const atoms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hits: Hit[] = [];
  for (const task of view.workspace.tasks.values()) {
    if (!filter(task)) continue;
    if (!atoms.length) {
      hits.push({ task, score: 0, matches: [] });
      continue;
    }
    const fields = haystacks(view, task);
    let total = 0;
    const matches: FieldMatch[] = [];
    let ok = true;
    for (const atom of atoms) {
      let best: { score: number; field: Field; indices: number[] } | undefined;
      for (const field of fields) {
        const scored = fuzzyScore(atom, field.text);
        // Prefer exact id-like hits in metadata; title ties win over others.
        const weight = field.kind === "meta" || field.kind === "seq" ? 1.1 : field.kind === "title" ? 1.05 : 1;
        if (scored && (!best || scored.score * weight > best.score)) {
          best = { score: scored.score * weight, field, indices: scored.indices };
        }
      }
      if (!best) {
        ok = false;
        break;
      }
      total += best.score;
      const existing = matches.find((m) => m.kind === best.field.kind && m.key === best.field.key && m.text === best.field.text);
      if (existing) existing.indices = [...new Set([...existing.indices, ...best.indices])].sort((a, b) => a - b);
      else matches.push({ kind: best.field.kind, ...(best.field.key !== undefined ? { key: best.field.key } : {}), text: best.field.text, indices: best.indices });
    }
    if (ok) hits.push({ task, score: Math.round(total), matches });
  }
  hits.sort((a, b) => b.score - a.score || a.task.seq - b.task.seq || compare(a.task.id, b.task.id));
  return hits;
}

/** Splits `text` into runs for highlighting matched `indices`. */
export function highlightRuns(text: string, indices: readonly number[]): { text: string; hit: boolean }[] {
  const set = new Set(indices);
  const runs: { text: string; hit: boolean }[] = [];
  for (let i = 0; i < text.length; i++) {
    const hit = set.has(i);
    const last = runs[runs.length - 1];
    if (last && last.hit === hit) last.text += text[i];
    else runs.push({ text: text[i]!, hit });
  }
  return runs;
}
