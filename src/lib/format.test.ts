import { expect, test } from "vitest";
import { costLabel, formatMoney, formatPct, formatTokens } from "./format";
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
