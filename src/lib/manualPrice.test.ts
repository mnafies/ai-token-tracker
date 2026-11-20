import { expect, test } from "vitest";
import { per1MToPerToken as t } from "./format";
import { manualPrices } from "./manualPrice";

test("converts all four prices from per 1M to per token", () => {
  expect(manualPrices({ prompt: "3", completion: "15", cacheRead: "0,3", cacheWrite: "3.75" })).toEqual({
    prompt: t(3), completion: t(15), cacheRead: t(0.3), cacheWrite: t(3.75),
  });
});
test("empty cache prices are null (cache tokens then use the input price)", () => {
  expect(manualPrices({ prompt: "1", completion: "2" })).toEqual({ prompt: t(1), completion: t(2), cacheRead: null, cacheWrite: null });
  expect(manualPrices({ prompt: "1", completion: "2", cacheRead: " ", cacheWrite: "" })).toEqual({
    prompt: t(1), completion: t(2), cacheRead: null, cacheWrite: null,
  });
});
test("missing input/output or any invalid number is an error", () => {
  expect(typeof manualPrices({ prompt: "", completion: "2" })).toBe("string");
  expect(typeof manualPrices({ prompt: "1" })).toBe("string");
  expect(typeof manualPrices({ prompt: "1", completion: "-2" })).toBe("string");
  expect(typeof manualPrices({ prompt: "1", completion: "2", cacheRead: "abc" })).toBe("string");
  expect(typeof manualPrices({ prompt: "1", completion: "2", cacheWrite: "-1" })).toBe("string");
});
