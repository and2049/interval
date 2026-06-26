import { describe, expect, test } from "bun:test";
import type { ReplayWeatherSection } from "../../../shared/types/api";
import { hasWeatherSample, weatherMetrics } from "./weatherDisplay";

describe("weather display helpers", () => {
  test("formats compact weather metrics from a sample", () => {
    const metrics = weatherMetrics(weather({ air_temp: 22.25, track_temp: 31.7, humidity: 42, wind_speed: 1.24 }));
    expect(metrics).toEqual([
      { label: "Air", value: "22.3C" },
      { label: "Track", value: "31.7C" },
      { label: "Humidity", value: "42%" },
      { label: "Wind", value: "1.2 m/s" }
    ]);
  });

  test("detects missing weather samples separately from missing fields", () => {
    expect(hasWeatherSample(weather({}))).toBe(true);
    expect(weatherMetrics(weather({})).map((metric) => metric.value)).toEqual(["--", "--", "--", "--"]);
    expect(hasWeatherSample({ sample: null, quality: "missing" })).toBe(false);
  });
});

function weather(sample: ReplayWeatherSection["sample"]): ReplayWeatherSection {
  return {
    sample,
    quality: "ready"
  };
}
