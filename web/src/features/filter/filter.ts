// Entry filter (timeline and Reports): selected project and tag ids, kept in
// the page URL.

import type { Uuid, Workspace } from "@tt/domain";

/** Project id, or this value for tasks without a project. */
export const NO_PROJECT = "none";

export interface EntryFilter {
  /** Project ids or `NO_PROJECT`; empty means any. */
  projects: string[];
  /** Tag ids; empty means any. */
  tags: Uuid[];
}

export const EMPTY_FILTER: EntryFilter = { projects: [], tags: [] };

export function filterActive(filter: EntryFilter): boolean {
  return filter.projects.length > 0 || filter.tags.length > 0;
}

export function filterCount(filter: EntryFilter): number {
  return filter.projects.length + filter.tags.length;
}

const list = (value: string | null) => (value ? value.split(",").filter(Boolean) : []);

/** Reads `project` and `tag`; ids unknown to the workspace are dropped. */
export function parseFilter(params: URLSearchParams, workspace: Workspace): EntryFilter {
  const unique = (ids: string[]) => [...new Set(ids)];
  return {
    projects: unique(list(params.get("project"))).filter((id) => id === NO_PROJECT || workspace.projects.has(id)),
    tags: unique(list(params.get("tag"))).filter((id) => workspace.tags.has(id)),
  };
}

/** Writes the filter onto `params` (mutates), removing empty keys. */
export function writeFilter(params: URLSearchParams, filter: EntryFilter): URLSearchParams {
  if (filter.projects.length) params.set("project", filter.projects.join(","));
  else params.delete("project");
  if (filter.tags.length) params.set("tag", filter.tags.join(","));
  else params.delete("tag");
  return params;
}
