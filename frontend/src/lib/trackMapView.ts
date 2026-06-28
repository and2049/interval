import type {
  DriverSnapshot,
  MapMode,
  TrackGeometry,
  TrackPositionSample
} from "../../../shared/types/api";
import {
  hasUsableGeometry,
  pointAtRelativeDistance,
  pointAtRelativeDistanceLookup,
  scalePoint,
  type TrackPointLookup
} from "./trackGeometry";

export interface TrackDistanceMarker {
  label: string;
  point: { x: number; y: number };
}

export interface TrackDriverDot {
  code: string;
  color: string;
  driver_number: number;
  point: { x: number; y: number };
  source: TrackPositionSample["source"];
  quality: TrackPositionSample["quality"];
  label: string;
  isLeader: boolean;
  isOut: boolean;
  isStale: boolean;
  opacity: number;
  radius: number;
  showCode: boolean;
}

export type TrackMapRenderMode = "real" | "schematic" | "pending" | "error";

export function hasRealTrackGeometry(geometry?: TrackGeometry): geometry is TrackGeometry {
  return Boolean(geometry && geometry.quality === "ready" && hasUsableGeometry(geometry.centerline));
}

export function trackMapRenderMode(
  mapMode: MapMode,
  geometry?: TrackGeometry,
  options: { error?: unknown } = {}
): TrackMapRenderMode {
  if (hasRealTrackGeometry(geometry)) return "real";
  if (mapMode !== "schematic" && options.error) return "error";
  return mapMode === "schematic" ? "schematic" : "pending";
}

export function trackMapPlaceholder(mode: TrackMapRenderMode): { label: string; detail: string } {
  return mode === "error"
    ? {
        label: "TRACK GEOMETRY UNAVAILABLE",
        detail: "MAP POSITIONS PAUSED"
      }
    : {
        label: "LOADING TRACK GEOMETRY",
        detail: "MAP POSITIONS PAUSED"
      };
}

export function displayTrackPoint(
  position: Pick<TrackPositionSample, "x" | "y">,
  geometry?: TrackGeometry
) {
  if (hasRealTrackGeometry(geometry)) {
    return scalePoint(position, geometry.bounds);
  }
  return {
    x: clamp(position.x, 5, 95),
    y: clamp(position.y, 8, 92)
  };
}

export function distanceMarkers(geometry?: TrackGeometry): TrackDistanceMarker[] {
  if (!hasRealTrackGeometry(geometry)) return [];
  const length = geometry.circuit_length ?? 0;
  if (length <= 0) return [];
  const markerCount = Math.min(6, Math.floor(length / 1000));
  const markers: TrackDistanceMarker[] = [];
  for (let index = 1; index <= markerCount; index += 1) {
    const point = pointAtRelativeDistance(geometry.centerline, (index * 1000) / length);
    if (point) markers.push({ label: `${index}K`, point });
  }
  return markers;
}

export function driverDots(
  positions: TrackPositionSample[],
  timingRows: DriverSnapshot[],
  geometry?: TrackGeometry
): TrackDriverDot[] {
  const drivers = new Map(timingRows.map((row) => [row.driver.driver_number, row.driver]));
  const ranks = new Map(timingRows.map((row) => [row.driver.driver_number, row.position]));
  const statuses = new Map(timingRows.map((row) => [row.driver.driver_number, row.status]));
  const leaderNumber = timingRows[0]?.driver.driver_number;
  return positions.map((position) => {
    const driver = drivers.get(position.driver_number);
    const rank = ranks.get(position.driver_number);
    const isOut = statuses.get(position.driver_number) === "out";
    const isStale = position.quality === "stale" || position.stale_seconds != null;
    return {
      code: driver?.code ?? String(position.driver_number),
      color: driver ? `#${driver.team_colour}` : "#2cf5bf",
      driver_number: position.driver_number,
      point: displayTrackPoint(position, geometry),
      source: position.source,
      quality: position.quality,
      label: driverDotLabel(driver?.code ?? String(position.driver_number), position, isOut),
      isLeader: position.driver_number === leaderNumber,
      isOut,
      isStale,
      opacity: isOut ? 0.38 : isStale ? 0.52 : 1,
      radius: isOut || isStale ? 1.05 : position.driver_number === leaderNumber ? 2.05 : 1.55,
      showCode: rank != null && rank <= 3
    };
  });
}

export function interpolateTrackPositions(
  from: TrackPositionSample[] | undefined,
  to: TrackPositionSample[],
  progress: number,
  geometry?: TrackGeometry,
  centerlineLookup?: TrackPointLookup
): TrackPositionSample[] {
  if (!from?.length) return to;

  const ratio = clamp(progress, 0, 1);
  if (ratio >= 1) return to;
  if (ratio <= 0) return to.map((position) => from.find((row) => row.driver_number === position.driver_number) ?? position);

  const previousByDriver = new Map(from.map((position) => [position.driver_number, position]));
  return to.map((next) => {
    const previous = previousByDriver.get(next.driver_number);
    if (!previous || !canInterpolatePosition(previous) || !canInterpolatePosition(next)) {
      return next;
    }

    const relativeDistance = interpolateRelativeDistance(
      previous.relative_distance,
      next.relative_distance,
      ratio
    );
    const point = pointForRelativeDistance(relativeDistance, geometry, centerlineLookup);

    return {
      ...next,
      x: point?.x ?? interpolateNumber(previous.x, next.x, ratio),
      y: point?.y ?? interpolateNumber(previous.y, next.y, ratio),
      z: interpolateOptionalNumber(previous.z, next.z, ratio),
      relative_distance: relativeDistance ?? next.relative_distance
    };
  });
}

function pointForRelativeDistance(
  relativeDistance: number | undefined,
  geometry?: TrackGeometry,
  centerlineLookup?: TrackPointLookup
) {
  if (relativeDistance == null) return undefined;
  if (centerlineLookup) return pointAtRelativeDistanceLookup(centerlineLookup, relativeDistance);
  return hasRealTrackGeometry(geometry)
    ? pointAtRelativeDistance(geometry.centerline, relativeDistance)
    : undefined;
}

export function startFinishLine(geometry?: TrackGeometry) {
  if (!hasRealTrackGeometry(geometry)) return undefined;
  const start = geometry.centerline[0];
  const inner = geometry.inner_edge[0];
  const outer = geometry.outer_edge[0];
  if (!start || !inner || !outer) return undefined;
  return {
    start: scalePoint(start, geometry.bounds),
    inner: scalePoint(inner, geometry.bounds),
    outer: scalePoint(outer, geometry.bounds)
  };
}

function canInterpolatePosition(position: Pick<TrackPositionSample, "x" | "y">) {
  return Number.isFinite(position.x) && Number.isFinite(position.y);
}

function interpolateNumber(from: number, to: number, progress: number) {
  return from + (to - from) * progress;
}

function interpolateOptionalNumber(
  from: number | null | undefined,
  to: number | null | undefined,
  progress: number
) {
  if (from == null && to == null) return to;
  if (from == null) return to;
  if (to == null) return from;
  if (!Number.isFinite(from) || !Number.isFinite(to)) return to;
  return interpolateNumber(from, to, progress);
}

function interpolateRelativeDistance(
  from: number | null | undefined,
  to: number | null | undefined,
  progress: number
) {
  if (from == null || to == null || !Number.isFinite(from) || !Number.isFinite(to)) {
    return undefined;
  }

  const normalizedFrom = wrapUnit(from);
  let normalizedTo = wrapUnit(to);
  if (normalizedTo < normalizedFrom && normalizedFrom - normalizedTo > 0.5) {
    normalizedTo += 1;
  }

  return wrapUnit(interpolateNumber(normalizedFrom, normalizedTo, progress));
}

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min;
  return Math.max(min, Math.min(max, value));
}

function wrapUnit(value: number) {
  return ((value % 1) + 1) % 1;
}

function driverDotLabel(code: string, position: TrackPositionSample, isOut = false): string {
  const stale = position.stale_seconds == null ? "" : `, ${position.stale_seconds.toFixed(0)}s stale`;
  const status = isOut ? ", out" : "";
  return `${code}: ${position.source}/${position.quality}${stale}${status}`;
}
