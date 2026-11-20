import { expect, test } from "vitest";
import { OTHER, buildChart } from "./chart";
import type { TrendBucket } from "../types";

const totals = { costUsd: 0, unpricedTokens: 0, tokens: 0, input: 0, output: 0, reasoning: 0, cacheRead: 0, cacheWrite: 0, events: 0 };
const bucket = (label: string, by: [string, number][]): TrendBucket => ({ label, startMs: 0, totals, by });

test("buildChart keeps top 8 by total and merges the rest into Lainnya", () => {
  const names = Array.from({ length: 10 }, (_, i) => `m${i}`);
  // m0 largest ... m9 smallest
  const by = names.map((n, i) => [n, 10 - i] as [string, number]);
  const { rows, series } = buildChart([bucket("W1", by), bucket("W2", [["m9", 1]])], "model");
  expect(series.map((s) => s.key)).toEqual([...names.slice(0, 8), OTHER]);
  expect(rows[0]).toMatchObject({ label: "W1", m0: 10, [OTHER]: 2 + 1 });
  expect(rows[1]).toEqual({ label: "W2", [OTHER]: 1 });
});
test("buildChart has no Lainnya with <= 8 series and uses label mapper", () => {
  const { series } = buildChart([bucket("W1", [["claude-code", 2], ["codex", 1]])], "agent", (k) => k.toUpperCase());
  expect(series.map((s) => s.key)).toEqual(["claude-code", "codex"]);
  expect(series.map((s) => s.name)).toEqual(["CLAUDE-CODE", "CODEX"]);
});
