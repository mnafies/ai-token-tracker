import { expect, test } from "vitest";
import { customRange } from "./range";

test("customRange uses local midnight start and next local midnight end (inclusive end date)", () => {
  expect(customRange("2026-03-02", "2026-03-04")).toEqual({
    kind: "custom",
    startMs: new Date(2026, 2, 2).getTime(),
    endMs: new Date(2026, 2, 5).getTime(),
  });
});
test("customRange single day spans one day", () => {
  expect(customRange("2026-03-02", "2026-03-02")).toEqual({
    kind: "custom", startMs: new Date(2026, 2, 2).getTime(), endMs: new Date(2026, 2, 3).getTime(),
  });
});
test("customRange rejects start after end and empty input", () => {
  expect(customRange("2026-03-05", "2026-03-04")).toBeNull();
  expect(customRange("", "2026-03-04")).toBeNull();
  expect(customRange("2026-03-04", "")).toBeNull();
});
