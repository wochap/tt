// Start grid + duration grid model shared by Settings, the timeline grid chip
// and Reports: a start grid must divide the duration grid; a duration change
// drops the start to the largest valid value.

import { validStartGrids } from "@tt/domain";

export type StartGrid = 0 | 5 | 10 | 15 | 30 | 60;
export type DurationGrid = 5 | 10 | 15 | 30 | 60;

export const START_GRIDS: readonly StartGrid[] = [0, 5, 10, 15, 30, 60];
export const DURATION_GRIDS: readonly DurationGrid[] = [5, 10, 15, 30, 60];

export interface Grids {
  start: StartGrid;
  duration: DurationGrid;
}

/** Start grids (minutes, 0 = off) valid for `duration` minutes. */
export function startOptions(duration: DurationGrid): StartGrid[] {
  return validStartGrids(duration * 60).map((s) => (s / 60) as StartGrid);
}

/** `grids` with `duration`, the start dropped to the largest valid value when needed. */
export function withDuration(grids: Grids, duration: DurationGrid): Grids {
  const valid = startOptions(duration);
  const start = valid.includes(grids.start) ? grids.start : (Math.max(...valid) as StartGrid);
  return { start, duration };
}

export function gridLabel(minutes: number): string {
  return minutes === 0 ? "off" : `${minutes}m`;
}

/**
 * Entry sheet round chip: the start goes down to the start grid (to the minute
 * when off) and, when stopped, the length from the new start to the old end is
 * rounded up to the duration grid.
 */
export function roundSpan(start: number, end: number | null, grids: Grids): { start: number; end?: number } {
  const startStep = Math.max(grids.start, 1) * 60_000;
  const lengthStep = grids.duration * 60_000;
  const newStart = Math.floor(start / startStep) * startStep;
  if (end === null) return { start: newStart };
  return { start: newStart, end: newStart + Math.max(Math.ceil((end - newStart) / lengthStep), 1) * lengthStep };
}
