import type { PeriodSpec } from "../types";

/** "YYYY-MM-DD" -> epoch ms at local midnight. */
function localMidnight(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).getTime();
}

/** Inclusive start/end dates ("YYYY-MM-DD") -> [local midnight, next local midnight); null if empty or inverted. */
export function customRange(start: string, end: string): PeriodSpec | null {
  if (!start || !end) return null;
  const startMs = localMidnight(start);
  const [y, m, d] = end.split("-").map(Number);
  const endMs = new Date(y, m - 1, d + 1).getTime();
  return endMs > startMs ? { kind: "custom", startMs, endMs } : null;
}
