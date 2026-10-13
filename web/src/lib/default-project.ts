// The sticky default project of text-created tasks: the last project a task was
// created with on this device, while the setting is on and the project exists.

import type { Workspace } from "@tt/domain";

import { useView } from "@/data/react";

import { useLastProject } from "./device-state.ts";
import { useSettings } from "./settings.ts";

/** Name of the default project, or undefined (setting off, nothing remembered, project deleted). */
export function useDefaultProject(): string | undefined {
  const { stickyProject } = useSettings();
  const id = useLastProject();
  const { workspace } = useView();
  return stickyProject && id ? defaultProjectName(workspace, id) : undefined;
}

export function defaultProjectName(workspace: Pick<Workspace, "projects">, id: string): string | undefined {
  return workspace.projects.get(id)?.name;
}
