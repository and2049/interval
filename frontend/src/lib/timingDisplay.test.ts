import { describe, expect, test } from "bun:test";
import type { DriverSnapshot } from "../../../shared/types/api";
import { compoundAbbreviation } from "./formatters";
import { gapLabel, hasTimingRows, intervalLabel, sectorCells } from "./timingDisplay";

describe("sectorCells", () => {
  test("pads missing sector cells to a stable three-column display", () => {
    expect(sectorCells([])).toEqual([undefined, undefined, undefined]);
    expect(sectorCells([{ number: 1, duration: 29.1, status: "normal" }])).toEqual([
      { number: 1, duration: 29.1, status: "normal" },
      undefined,
      undefined
    ]);
  });

  test("trims extra sector samples to the display columns", () => {
    expect(
      sectorCells([
        { number: 1, duration: 1, status: "normal" },
        { number: 2, duration: 2, status: "normal" },
        { number: 3, duration: 3, status: "normal" },
        { number: 4, duration: 4, status: "normal" }
      ])
    ).toHaveLength(3);
  });
});

describe("compoundAbbreviation", () => {
  test("uses compact tyre labels with an explicit unknown fallback", () => {
    expect(compoundAbbreviation("SOFT")).toBe("S");
    expect(compoundAbbreviation("INTERMEDIATE")).toBe("I");
    expect(compoundAbbreviation("UNKNOWN")).toBe("--");
  });
});

describe("timing row display helpers", () => {
  test("detects empty timing tower data", () => {
    expect(hasTimingRows([])).toBe(false);
    expect(hasTimingRows([row()])).toBe(true);
  });

  test("formats leader and missing gap labels", () => {
    expect(gapLabel(1, null)).toBe("LEADER");
    expect(gapLabel(4, "+4.2")).toBe("+4.2");
    expect(gapLabel(4, null)).toBe("--");
  });

  test("renders provided intervals and falls back for missing values", () => {
    expect(intervalLabel("+1.234")).toBe("+1.234");
    expect(intervalLabel(null)).toBe("--");
  });
});

function row(): DriverSnapshot {
  return {
    driver: {
      driver_number: 1,
      code: "VER",
      full_name: "Max Verstappen",
      team_name: "Red Bull Racing",
      team_colour: "3671C6"
    },
    position: 1,
    rank_source: "open_f1_position",
    gap_to_leader: null,
    interval: null,
    lap: 1,
    last_lap: null,
    compound: "MEDIUM",
    stint_age: null,
    sectors: [],
    in_pit: false,
    status: "on_track"
  };
}
