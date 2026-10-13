// Timeline filter: selected project and tag ids, kept in the page URL.

import type { Uuid, Workspace } from "@tt/domain";

/** Project id, or this value for tasks without a project. */
export const NO_PROJECT = "none";

export interface TimelineFilter {
  /** Project ids or `NO_PROJECT`; empty means any. */
  projects: string[];
  /** Tag ids; empty means any. */
  tags: Uuid[];
}

export const EMPTY_FILTER: TimelineFilter = { projects: [], tags: [] };

export function filterActive(filter: TimelineFilter): boolean {
  return filter.projects.length > 0 || filter.tags.length > 0;
}

export function filterCount(filter: TimelineFilter): number {
  return filter.projects.length + filter.tags.length;
}

const list = (value: string | null) => (value ? value.split(",").filter(Boolean) : []);

/** Reads `project` and `tag`; ids unknown to the workspace are dropped. */
export function parseFilter(params: URLSearchParams, workspace: Workspace): TimelineFilter {
  const unique = (ids: string[]) => [...new Set(ids)];
  return {
    projects: unique(list(params.get("project"))).filter((id) => id === NO_PROJECT || workspace.projects.has(id)),
    tags: unique(list(params.get("tag"))).filter((id) => workspace.tags.has(id)),
  };
}

/** Writes the filter onto `params` (mutates), removing empty keys. */
export function writeFilter(params: URLSearchParams, filter: TimelineFilter): URLSearchParams {
  if (filter.projects.length) params.set("project", filter.projects.join(","));
  else params.delete("project");
  if (filter.tags.length) params.set("tag", filter.tags.join(","));
  else params.delete("tag");
  return params;
}
