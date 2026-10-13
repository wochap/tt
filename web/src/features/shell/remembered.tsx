import { Navigate, useSearchParams } from "react-router";

import { formatDate } from "@tt/domain";

import { rememberedDate, useViewState } from "@/lib/device-state";
import { usePhone } from "@/lib/media";
import { useTz } from "@/lib/settings";

import { TasksPage } from "../tasks/TasksPage.tsx";
import { todayIn } from "../timeline/geometry.ts";

/** Bare /timeline: reopen the last range, and its date only on the day it was saved. */
export function RememberedTimeline() {
  const tz = useTz();
  const state = useViewState();
  const phone = usePhone();
  const date = phone ? undefined : rememberedDate(state, todayIn(tz, Date.now()));
  const range = phone ? "day" : state.range;
  return <Navigate to={`/timeline/${range}${date ? `?date=${formatDate(date)}` : ""}`} replace />;
}

/** Bare /tasks: reopen as a board when that was the last mode. An explicit `view` wins. */
export function RememberedTasks() {
  const [params] = useSearchParams();
  const state = useViewState();
  const phone = usePhone();
  if (!phone && !params.has("view") && state.tasksMode === "board") {
    const q = new URLSearchParams(params);
    q.set("view", "board");
    return <Navigate to={`/tasks?${q.toString()}`} replace />;
  }
  return <TasksPage />;
}
