import { describe, expect, test } from "bun:test";
import { parseSelectNumber } from "./selectField";

describe("parseSelectNumber", () => {
  test("returns a numeric select value", () => {
    expect(parseSelectNumber("9472")).toBe(9472);
  });

  test("ignores empty or invalid select values", () => {
    expect(parseSelectNumber("")).toBeUndefined();
    expect(parseSelectNumber("   ")).toBeUndefined();
    expect(parseSelectNumber("session")).toBeUndefined();
  });
});
