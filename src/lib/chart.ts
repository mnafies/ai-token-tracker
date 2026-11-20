import type { TrendBucket } from "../types";
import { OTHER_COLOR, agentColor, seriesColor } from "./colors";

export interface StackRow { label: string; [series: string]: number | string }
export interface StackSeries { key: string; name: string; color: string }

export const MAX_SERIES = 8;
export const OTHER = "Lainnya";

/** Keeps the top series by total cost; the rest are merged into "Lainnya". Zero values are omitted. */
export function buildChart(
  buckets: TrendBucket[], split: "agent" | "model" | "project", labelOf: (k: string) => string = (k) => k,
): { rows: StackRow[]; series: StackSeries[] } {
  const totals = new Map<string, number>();
  for (const b of buckets) for (const [k, v] of b.by) totals.set(k, (totals.get(k) ?? 0) + v);
  const ranked = [...totals.entries()].sort((a, b) => b[1] - a[1]).map(([k]) => k);
  const top = ranked.slice(0, MAX_SERIES);
  const rows: StackRow[] = buckets.map((b) => {
    const row: StackRow = { label: b.label };
    for (const [k, v] of b.by) {
      if (!(v > 0)) continue;
      const key = top.includes(k) ? k : OTHER;
      row[key] = ((row[key] as number | undefined) ?? 0) + v;
    }
    return row;
  });
  const series = top.map((k, i) => ({
    key: k, name: split === "agent" ? labelOf(k) : k, color: split === "agent" ? agentColor(k) : seriesColor(i),
  }));
  if (ranked.length > MAX_SERIES) series.push({ key: OTHER, name: OTHER, color: OTHER_COLOR });
  return { rows, series };
}
