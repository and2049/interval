import { describe, expect, test } from "bun:test";
import {
  formatEventClock,
  formatLapTime,
  formatPercent,
  formatRaceClock,
  formatSpeed,
  formatTemperature
} from "./formatters";

describe("formatRaceClock", () => {
  test("clamps negative race time to zero", () => {
    expect(formatRaceClock(-4)).toBe("00:00");
  });

  test("formats elapsed seconds as minute clock", () => {
    expect(formatRaceClock(125.9)).toBe("02:05");
  });
});

describe("formatEventClock", () => {
  test("formats pre-session events with T-minus clock", () => {
    expect(formatEventClock(-176.4)).toBe("T-02:57");
  });

  test("formats in-session events as race clock", () => {
    expect(formatEventClock(61.8)).toBe("01:01");
  });
});

describe("formatLapTime", () => {
  test("formats null and NaN as missing data", () => {
    expect(formatLapTime(null)).toBe("--");
    expect(formatLapTime(Number.NaN)).toBe("--");
  });

  test("formats lap durations with millisecond precision", () => {
    expect(formatLapTime(90.1234)).toBe("1:30.123");
  });
});

describe("weather formatters", () => {
  test("render missing values without dangling units", () => {
    expect(formatTemperature(null)).toBe("--");
    expect(formatPercent(undefined)).toBe("--");
    expect(formatSpeed(Number.NaN)).toBe("--");
  });

  test("render values with compact units", () => {
    expect(formatTemperature(23.74)).toBe("23.7C");
    expect(formatPercent(49.4)).toBe("49%");
    expect(formatSpeed(1.56)).toBe("1.6 m/s");
  });
});
