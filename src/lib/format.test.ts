import { expect, test } from "vitest";
import { costLabel, formatDuration, formatMoney, formatPct, formatPer1M, formatTokens, per1MToPerToken, perTokenToPer1M, priceStatusLabel, projectName } from "./format";
import type { Settings } from "../types";

const base: Settings = { currency: "usd", idrRate: null, launchAtLogin: true, processPollSecs: 5, scanRoots: [] };

test("formatTokens scales", () => {
  expect(formatTokens(950)).toBe("950");
  expect(formatTokens(12300)).toBe("12.3K");
  expect(formatTokens(4560000)).toBe("4.56M");
  expect(formatTokens(1.2e9)).toBe("1.2B");
  expect(formatTokens(1000)).toBe("1K");
  expect(formatTokens(0)).toBe("0");
  expect(formatTokens(999999)).toBe("1M");
  expect(formatTokens(123456)).toBe("123K");
});
test("formatMoney usd", () => {
  expect(formatMoney(4.126, base)).toBe("$4.13");
});
test("formatMoney idr", () => {
  expect(formatMoney(4.12, { ...base, currency: "idr", idrRate: 16000 })).toBe("Rp 65.920");
  expect(formatMoney(1000, { ...base, currency: "idr", idrRate: 16000 })).toBe("Rp 16.000.000");
});
test("formatMoney idr without rate falls back", () => {
  expect(formatMoney(4.12, { ...base, currency: "idr", idrRate: null })).toBe("$4.12");
  expect(formatMoney(4.12, { ...base, currency: "idr", idrRate: 0 })).toBe("$4.12");
});
test("formatPct arrows", () => {
  expect(formatPct(12.04)).toBe("▲12%");
  expect(formatPct(-5)).toBe("▼5%");
  expect(formatPct(0)).toBe("▲0%");
  expect(formatPct(-0.2)).toBe("▲0%");
  expect(formatPct(null)).toBe("—");
});
test("costLabel mentions unpriced", () => {
  expect(costLabel(1, 500, base)).toBe("$1.00 + 500 token tanpa harga");
  expect(costLabel(1, 0, base)).toBe("$1.00");
});

test("formatDuration", () => {
  expect(formatDuration(0, 65 * 60000)).toBe("1j 5m");
  expect(formatDuration(0, 12 * 60000 + 5000)).toBe("12m");
  expect(formatDuration(0, 45000)).toBe("45d");
  expect(formatDuration(0, 3600000)).toBe("1j");
  expect(formatDuration(null, 5)).toBe("—");
  expect(formatDuration(5, null)).toBe("—");
  expect(formatDuration(10, 5)).toBe("—");
});

test("price helpers", () => {
  expect(formatPer1M(0.000003)).toBe("$3.00");
  expect(formatPer1M(null)).toBe("—");
  expect(priceStatusLabel("mapped")).toBe("Dipetakan");
  expect(priceStatusLabel("unknown")).toBe("Tidak diketahui");
  expect(priceStatusLabel("zzz")).toBe("zzz");
});

test("projectName is the last path segment", () => {
  expect(projectName("/a/b/proj")).toBe("proj");
  expect(projectName("/a/b/proj/")).toBe("proj");
  expect(projectName("/")).toBe("/");
  expect(projectName("plain")).toBe("plain");
});

test("per1MToPerToken divides by one million", () => {
  expect(per1MToPerToken(3)).toBe(0.000003);
  expect(per1MToPerToken(0)).toBe(0);
});

test("perTokenToPer1M trims float noise to 4 decimals", () => {
  expect(perTokenToPer1M(0.000003)).toBe("3");
  expect(perTokenToPer1M(1.5e-7)).toBe("0.15");
  expect(perTokenToPer1M(0.0000012345678)).toBe("1.2346");
  expect(perTokenToPer1M(0.1 * 3 / 1e6)).toBe("0.3");
});
