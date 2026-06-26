import { describe, expect, test } from "bun:test";
import type {
  DriverSnapshot,
  TrackGeometry,
  TrackPositionSample
} from "../../../shared/types/api";
import {
  displayTrackPoint,
  distanceMarkers,
  driverDots,
  hasRealTrackGeometry,
  startFinishLine,
  trackMapPlaceholder,
  trackMapRenderMode
} from "./trackMapView";

describe("hasRealTrackGeometry", () => {
  test("requires ready geometry with a usable centerline", () => {
    expect(hasRealTrackGeometry(geometry())).toBe(true);
    expect(hasRealTrackGeometry({ ...geometry(), quality: "schematic" })).toBe(false);
    expect(hasRealTrackGeometry({ ...geometry(), centerline: [] })).toBe(false);
  });
});

describe("trackMapRenderMode", () => {
  test("uses real geometry when a usable track is available", () => {
    expect(trackMapRenderMode("projected", geometry())).toBe("real");
    expect(trackMapRenderMode("gps", geometry())).toBe("real");
  });

  test("keeps schematic fallback only for schematic snapshots", () => {
    expect(trackMapRenderMode("schematic")).toBe("schematic");
    expect(trackMapRenderMode("schematic", { ...geometry(), quality: "schematic" })).toBe("schematic");
  });

  test("marks projected or gps snapshots as pending while geometry loads", () => {
    expect(trackMapRenderMode("projected")).toBe("pending");
    expect(trackMapRenderMode("gps", { ...geometry(), centerline: [] })).toBe("pending");
  });

  test("marks projected or gps snapshots as failed when geometry loading errors", () => {
    expect(trackMapRenderMode("projected", undefined, { error: new Error("missing") })).toBe("error");
    expect(trackMapRenderMode("gps", undefined, { error: "missing" })).toBe("error");
    expect(trackMapRenderMode("schematic", undefined, { error: "missing" })).toBe("schematic");
  });
});

describe("trackMapPlaceholder", () => {
  test("labels pending and error placeholder states", () => {
    expect(trackMapPlaceholder("pending")).toEqual({
      label: "LOADING TRACK GEOMETRY",
      detail: "MAP POSITIONS PAUSED"
    });
    expect(trackMapPlaceholder("error")).toEqual({
      label: "TRACK GEOMETRY UNAVAILABLE",
      detail: "MAP POSITIONS PAUSED"
    });
  });
});

describe("displayTrackPoint", () => {
  test("scales real geometry points and clamps schematic coordinates", () => {
    expect(displayTrackPoint({ x: 100, y: 50 }, geometry())).toEqual({ x: 50, y: 50 });
    expect(displayTrackPoint({ x: -50, y: 500 })).toEqual({ x: 5, y: 92 });
  });
});

describe("distanceMarkers", () => {
  test("places kilometer markers on usable geometry only", () => {
    expect(distanceMarkers(geometry()).map((marker) => marker.label)).toEqual(["1K", "2K", "3K"]);
    expect(distanceMarkers({ ...geometry(), quality: "schematic" })).toEqual([]);
  });
});

describe("driverDots", () => {
  test("decorates positions with timing driver metadata and leader state", () => {
    const dots = driverDots(
      [position(1, 100, 50), position(4, 250, -20)],
      [row(1, "VER", "3671C6"), row(4, "NOR", "FF8000")],
      geometry()
    );

    expect(dots[0]).toMatchObject({
      code: "VER",
      color: "#3671C6",
      driver_number: 1,
      isLeader: true,
      source: "projected",
      quality: "projected",
      label: "VER: projected/projected",
      point: { x: 50, y: 50 }
    });
    expect(dots[1]).toMatchObject({
      code: "NOR",
      isLeader: false
    });
  });

  test("includes stale source metadata in the accessible dot label", () => {
    const dots = driverDots(
      [{ ...position(4, 100, 50), source: "interpolated", quality: "interpolated", stale_seconds: 12 }],
      [row(4, "NOR", "FF8000")],
      geometry()
    );

    expect(dots[0].label).toBe("NOR: interpolated/interpolated, 12s stale");
  });
});

describe("startFinishLine", () => {
  test("returns scaled start and edge points for real geometry", () => {
    expect(startFinishLine(geometry())).toEqual({
      start: { x: 5, y: 72.5 },
      inner: { x: 5, y: 73.4 },
      outer: { x: 5, y: 71.6 }
    });
    expect(startFinishLine({ ...geometry(), centerline: [] })).toBeUndefined();
  });
});

function geometry(): TrackGeometry {
  return {
    contract_version: "replay.v1",
    session_key: 9472,
    bounds: {
      min_x: 0,
      max_x: 200,
      min_y: 0,
      max_y: 100
    },
    centerline: [
      { x: 0, y: 0, cumulative_distance: 0, relative_distance: 0 },
      { x: 100, y: 50, cumulative_distance: 1500, relative_distance: 0.5 },
      { x: 200, y: 100, cumulative_distance: 3000, relative_distance: 1 }
    ],
    inner_edge: [
      { x: 0, y: -2, cumulative_distance: 0, relative_distance: 0 },
      { x: 100, y: 48, cumulative_distance: 1500, relative_distance: 0.5 },
      { x: 200, y: 98, cumulative_distance: 3000, relative_distance: 1 }
    ],
    outer_edge: [
      { x: 0, y: 2, cumulative_distance: 0, relative_distance: 0 },
      { x: 100, y: 52, cumulative_distance: 1500, relative_distance: 0.5 },
      { x: 200, y: 102, cumulative_distance: 3000, relative_distance: 1 }
    ],
    source: "curated_static",
    quality: "ready",
    map_mode: "projected",
    circuit_length: 3000,
    generated_at: ""
  };
}

function position(driverNumber: number, x: number, y: number): TrackPositionSample {
  return {
    driver_number: driverNumber,
    x,
    y,
    z: null,
    relative_distance: null,
    source: "projected",
    quality: "projected",
    stale_seconds: null
  };
}

function row(driverNumber: number, code: string, teamColour: string): DriverSnapshot {
  return {
    driver: {
      driver_number: driverNumber,
      code,
      full_name: code,
      team_name: "Team",
      team_colour: teamColour
    },
    position: driverNumber,
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
