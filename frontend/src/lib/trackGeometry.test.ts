import { describe, expect, test } from "bun:test";
import type { TrackBounds, TrackPoint } from "../../../shared/types/api";
import {
  closedRoadPath,
  createTrackPointLookup,
  pointAtRelativeDistance,
  pointAtRelativeDistanceLookup,
  pointsToPath,
  scalePoint
} from "./trackGeometry";

const bounds: TrackBounds = {
  min_x: 0,
  max_x: 200,
  min_y: 0,
  max_y: 100
};

const centerline: TrackPoint[] = [
  { x: 0, y: 0, cumulative_distance: 0, relative_distance: 0 },
  { x: 100, y: 50, cumulative_distance: 100, relative_distance: 0.5 },
  { x: 200, y: 100, cumulative_distance: 200, relative_distance: 1 }
];

describe("scalePoint", () => {
  test("preserves aspect ratio and centers the drawing area", () => {
    expect(scalePoint({ x: 0, y: 0 }, bounds)).toEqual({ x: 5, y: 72.5 });
    expect(scalePoint({ x: 200, y: 100 }, bounds)).toEqual({ x: 95, y: 27.5 });
  });
});

describe("pointsToPath", () => {
  test("builds a stable svg path from backend points", () => {
    expect(pointsToPath(centerline, bounds)).toBe(
      "M 5.00 72.50 L 50.00 50.00 L 95.00 27.50"
    );
  });
});

describe("closedRoadPath", () => {
  test("returns an empty path when road edges are not usable", () => {
    expect(closedRoadPath([], centerline, bounds)).toBe("");
  });

  test("closes outer and reversed inner edges into one road fill path", () => {
    expect(closedRoadPath(centerline, centerline, bounds).endsWith("Z")).toBe(true);
  });
});

describe("pointAtRelativeDistance", () => {
  test("interpolates between centerline points", () => {
    expect(pointAtRelativeDistance(centerline, 0.25)).toEqual({ x: 50, y: 25 });
  });

  test("uses a reusable lookup for repeated point interpolation", () => {
    const lookup = createTrackPointLookup(centerline);

    expect(lookup).toBeDefined();
    expect(pointAtRelativeDistanceLookup(lookup!, 0.75)).toEqual({ x: 150, y: 75 });
  });

  test("wraps negative and overflow distances", () => {
    expect(pointAtRelativeDistance(centerline, -0.75)).toEqual({ x: 50, y: 25 });
    expect(pointAtRelativeDistance(centerline, 1.25)).toEqual({ x: 50, y: 25 });
  });
});
