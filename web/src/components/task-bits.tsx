// Small pieces shared by lists, pickers and the timeline.

import type { Task, View } from "@tt/domain";
import { highlightRuns, taskProject, taskTags } from "@tt/domain";

import { Badge, ColorDot } from "@/components/ui/badge";
import { cssColor, cssTextColor, projectColor, projectColorName, taskColor, tagColor } from "@/lib/colors";
import { cn } from "@/lib/utils";

export function Seq({ seq, className }: { seq: number; className?: string }) {
  return <span className={cn("shrink-0 font-mono text-[10.5px] text-faint", className)}>#{seq}</span>;
}

export function TaskDot({ view, task, size = 8 }: { view: View; task: Task | undefined; size?: number }) {
  return <ColorDot color={taskColor(view.workspace, task)} size={size} />;
}

export function TagChips({ view, task, size = "sm", max }: { view: View; task: Task; size?: "xs" | "sm" | "md"; max?: number }) {
  const tags = taskTags(view.workspace, task);
  const shown = max === undefined ? tags : tags.slice(0, max);
  return (
    <>
      {shown.map((tag) => (
        <Badge key={tag.id} size={size} color={tagColor(tag)} textColor={cssTextColor(tag.color)}>
          {tag.name}
        </Badge>
      ))}
      {max !== undefined && tags.length > max && <span className="text-[10.5px] text-faint">+{tags.length - max}</span>}
    </>
  );
}

export function ProjectBadge({ view, task, className }: { view: View; task: Task; className?: string }) {
  const project = taskProject(view.workspace, task);
  if (!project) return null;
  const color = projectColor(project);
  return (
    <Badge size="md" color={color} textColor={cssTextColor(projectColorName(project))} strength={18} className={className}>
      <ColorDot color={color} size={7} />
      {project.name}
    </Badge>
  );
}

/** Text with matched character positions highlighted (search hits). */
export function Highlight({ text, indices, mark = "link" }: { text: string; indices?: readonly number[]; mark?: "link" | "yellow" }) {
  if (!indices?.length) return <>{text}</>;
  return (
    <>
      {highlightRuns(text, indices).map((run, i) =>
        run.hit ? (
          mark === "yellow" ? (
            <mark key={i} className="rounded-xs bg-[color-mix(in_srgb,var(--ctp-yellow)_35%,transparent)] px-[2px] text-fg">
              {run.text}
            </mark>
          ) : (
            <span key={i} className="text-link">
              {run.text}
            </span>
          )
        ) : (
          <span key={i}>{run.text}</span>
        ),
      )}
    </>
  );
}

export { cssColor };
