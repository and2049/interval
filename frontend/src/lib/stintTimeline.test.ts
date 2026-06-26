import { describe, expect, test } from "bun:test";
import { stintProgressDisplay } from "./stintTimeline";

describe("stintProgressDisplay", () => {
  test("marks missing or invalid stint ages as unknown", () => {
    expect(stintProgressDisplay(null)).toEqual({
      label: "Age --",
      widthPercent: 0,
      known: false
    });
    expect(stintProgressDisplay(Number.NaN)).toEqual({
      label: "Age --",
      widthPercent: 0,
      known: false
    });
    expect(stintProgressDisplay(-1)).toEqual({
      label: "Age --",
      widthPercent: 0,
      known: false
    });
  });

  test("formats known stint ages and clamps progress width", () => {
    expect(stintProgressDisplay(8.7)).toEqual({
      label: "Age 8",
      widthPercent: 34.8,
      known: true
    });
    expect(stintProgressDisplay(40)).toMatchObject({
      widthPercent: 100,
      known: true
    });
  });
});
