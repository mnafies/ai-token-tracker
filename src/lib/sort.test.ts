import { expect, test } from "vitest";
import { sortRows } from "./sort";

test("sorts numbers and strings, nulls last in both directions", () => {
  const rows = [{ v: 2 }, { v: null }, { v: 10 }, { v: 1 }];
  expect(sortRows(rows, (r) => r.v, "asc").map((r) => r.v)).toEqual([1, 2, 10, null]);
  expect(sortRows(rows, (r) => r.v, "desc").map((r) => r.v)).toEqual([10, 2, 1, null]);
  expect(sortRows([{ v: "b" }, { v: "a" }], (r) => r.v, "asc").map((r) => r.v)).toEqual(["a", "b"]);
});
test("does not mutate input", () => {
  const rows = [{ v: 2 }, { v: 1 }];
  sortRows(rows, (r) => r.v, "asc");
  expect(rows[0].v).toBe(2);
});
