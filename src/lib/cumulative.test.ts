import { expect, test } from "vitest";
import { buildCumulative } from "./cumulative";
import type { EventRow } from "../types";

const ev = (tsMs: number, costUsd: number | null, tokens = 10): EventRow => ({
  tsMs, model: "m", input: tokens, output: tokens, reasoning: 0, cacheRead: tokens, cacheWrite: tokens,
  costUsd, priceStatus: costUsd == null ? "unknown" : "matched", cwd: null,
});

test("cumulative sums priced costs in time order; null contributes 0", () => {
  const r = buildCumulative([ev(3000, 1), ev(1000, 0.5), ev(2000, null)]);
  expect(r.points.map((p) => p.cumulative)).toEqual([0.5, 0.5, 1.5]);
  expect(r.points.map((p) => p.index)).toEqual([1, 2, 3]);
  expect(r.unpricedTokens).toBe(40);
});
test("no unpriced events", () => {
  expect(buildCumulative([ev(1, 2)]).unpricedTokens).toBe(0);
  expect(buildCumulative([]).points).toEqual([]);
});
