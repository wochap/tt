// Entry and chip colors: project color first, then the first colored tag,
// otherwise neutral. Stored colors may be a Catppuccin name ("mauve"), a hex
// value, or absent (projects then get a stable color from their id).

import type { CSSProperties } from "react";

import type { Project, Tag, Task, Workspace } from "@tt/domain";

export const PALETTE = [
  "mauve",
  "peach",
  "teal",
  "blue",
  "pink",
  "sapphire",
  "green",
  "yellow",
  "lavender",
  "flamingo",
  "maroon",
  "sky",
  "red",
  "rosewater",
] as const;
export type PaletteName = (typeof PALETTE)[number];

export const NEUTRAL = "var(--c-ov1)";

function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return h >>> 0;
}

/** CSS color for a stored color value. */
export function cssColor(value: string | undefined): string | undefined {
  if (!value) return undefined;
  const name = value.trim().toLowerCase();
  if ((PALETTE as readonly string[]).includes(name)) return `var(--ctp-${name})`;
  if (/^#[0-9a-f]{3,8}$/i.test(name)) return name;
  return undefined;
}

/** CSS color for small text in that color (AA in both flavors). */
export function cssTextColor(value: string | undefined): string | undefined {
  if (!value) return undefined;
  const name = value.trim().toLowerCase();
  if ((PALETTE as readonly string[]).includes(name)) return `var(--fg-${name})`;
  return cssColor(value);
}

export function projectColorName(project: Project): string {
  return project.color && cssColor(project.color) ? project.color : PALETTE[hash(project.id) % 8]!;
}

export function projectColor(project: Project): string {
  return cssColor(projectColorName(project))!;
}

export function tagColor(tag: Tag): string | undefined {
  return cssColor(tag.color);
}

export function taskColorName(workspace: Workspace, task: Task | undefined): string | undefined {
  if (!task) return undefined;
  const project = task.project ? workspace.projects.get(task.project) : undefined;
  if (project) return projectColorName(project);
  for (const id of task.tags) {
    const tag = workspace.tags.get(id);
    if (tag?.color && cssColor(tag.color)) return tag.color;
  }
  return undefined;
}

export function taskColor(workspace: Workspace, task: Task | undefined): string {
  return cssColor(taskColorName(workspace, task)) ?? NEUTRAL;
}

export function taskTextColor(workspace: Workspace, task: Task | undefined): string {
  return cssTextColor(taskColorName(workspace, task)) ?? "var(--fg-neutral)";
}

/** Inline style of a tinted chip (tags, project badges). */
export function chipStyle(color: string | undefined, textColor?: string, strength = 20): CSSProperties {
  if (!color) return { background: "var(--c-s0)", color: "var(--c-sub1)" };
  return { background: `color-mix(in srgb, ${color} ${strength}%, var(--c-base))`, color: textColor ?? color };
}
