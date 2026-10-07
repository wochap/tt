// Lane layout for overlapping entries (`useLaneLayout` core). Entries are
// swept in start order into clusters of transitively overlapping intervals;
// each entry takes the first lane free at its start, and every entry of a
// cluster gets the cluster's lane count, so a column splits into N equal lanes.

import { compare, type Millis } from "./model.ts";

export interface LaneInput {
  id: string;
  start: Millis;
  /** `null` while running (extends to `now`). */
  end: Millis | null;
}

export interface LanePlacement {
  id: string;
  lane: number;
  lanes: number;
  start: Millis;
  end: Millis;
}

export function laneLayout(items: readonly LaneInput[], now: Millis): LanePlacement[] {
  const sorted = items
    .map((item) => ({ id: item.id, start: item.start, end: Math.max(item.start, item.end ?? now) }))
    .sort((a, b) => a.start - b.start || b.end - a.end || compare(a.id, b.id));
  const out: LanePlacement[] = [];
  let cluster: LanePlacement[] = [];
  let laneEnds: Millis[] = [];
  let clusterEnd = -Infinity;
  const flush = () => {
    for (const placement of cluster) placement.lanes = laneEnds.length;
    cluster = [];
    laneEnds = [];
    clusterEnd = -Infinity;
  };
  for (const item of sorted) {
    if (cluster.length && item.start >= clusterEnd) flush();
    let lane = laneEnds.findIndex((end) => end <= item.start);
    if (lane < 0) {
      lane = laneEnds.length;
      laneEnds.push(item.end);
    } else {
      laneEnds[lane] = item.end;
    }
    clusterEnd = Math.max(clusterEnd, item.end);
    const placement = { ...item, lane, lanes: 1 };
    cluster.push(placement);
    out.push(placement);
  }
  flush();
  return out;
}

/** CSS geometry for a placement: percentage left/width with a gutter in px. */
export function laneGeometry(lane: number, lanes: number, gutterPx = 3): { left: string; width: string } {
  const width = 100 / lanes;
  return {
    left: `calc(${lane * width}% + ${lane ? gutterPx : 0}px)`,
    width: `calc(${width}% - ${lanes > 1 ? gutterPx : 0}px)`,
  };
}
