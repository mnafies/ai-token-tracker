export type SortValue = number | string | null | undefined;

/** Stable sort by a key; null/undefined always last. Returns a new array. */
export function sortRows<T>(rows: T[], value: (r: T) => SortValue, dir: "asc" | "desc"): T[] {
  const sign = dir === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const x = value(a), y = value(b);
    if (x == null && y == null) return 0;
    if (x == null) return 1;
    if (y == null) return -1;
    if (typeof x === "number" && typeof y === "number") return sign * (x - y);
    return sign * String(x).localeCompare(String(y));
  });
}
