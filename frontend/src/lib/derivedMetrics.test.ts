import { describe, expect, test } from "bun:test";
import { derivedMetricRows } from "./derivedMetrics";

describe("derivedMetricRows", () => {
  test("decorates driver metrics with timing tower driver codes", () => {
    expect(
      derivedMetricRows(
        [
          {
            driver_number: 1,
            kind: "recent_pace",
            label: "3-lap avg",
            value: "96.936",
            trend: "stable"
          }
        ],
        [timingRow(1, "VER")]
      )
    ).toEqual([
      {
        driver: "VER",
        label: "3-lap avg",
        value: "96.936",
        trend: "stable"
      }
    ]);
  });

  test("falls back when a metric is session-wide or the driver is missing", () => {
    expect(
      derivedMetricRows(
        [
          {
            driver_number: null,
            kind: "pit_state",
            label: "pit lane",
            value: "closed",
            trend: "unknown"
          },
          {
            driver_number: 99,
            kind: "recent_pace",
            label: "3-lap avg",
            value: "100.000",
            trend: "degrading"
          }
        ],
        []
      ).map((row) => row.driver)
    ).toEqual(["--", "99"]);
  });
});

function timingRow(driverNumber: number, code: string) {
  return {
    driver: {
      driver_number: driverNumber,
      code,
      full_name: code,
      team_name: "Team",
      team_colour: "FFFFFF"
    },
    position: 1,
    rank_source: "openf1_position",
    gap_to_leader: null,
    interval: null,
    lap: 1,
    last_lap: null,
    compound: "UNKNOWN",
    stint_age: null,
    sectors: [],
    in_pit: false,
    status: "on_track"
  } as const;
}
