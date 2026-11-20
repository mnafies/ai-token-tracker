import type { EventRow } from "../types";

export interface CumulativePoint { index: number; tsMs: number; cumulative: number }

/** Cumulative cost per response in time order. Unpriced events (costUsd null) add 0 and are counted separately. */
export function buildCumulative(events: EventRow[]): { points: CumulativePoint[]; unpricedTokens: number } {
  const sorted = [...events].sort((a, b) => a.tsMs - b.tsMs);
  let sum = 0;
  let unpricedTokens = 0;
  const points = sorted.map((e, i) => {
    if (e.costUsd == null) unpricedTokens += e.input + e.output + e.cacheRead + e.cacheWrite;
    else sum += e.costUsd;
    return { index: i + 1, tsMs: e.tsMs, cumulative: sum };
  });
  return { points, unpricedTokens };
}
