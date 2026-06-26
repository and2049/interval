import type {
  DriverSnapshot,
  MapMode,
  TrackGeometry,
  TrackPositionSample
} from "../../../shared/types/api";
import { hasUsableGeometry, pointAtRelativeDistance, scalePoint } from "./trackGeometry";

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
  const leaderNumber = timingRows[0]?.driver.driver_number;
  return positions.map((position) => {
    const driver = drivers.get(position.driver_number);
    return {
      code: driver?.code ?? String(position.driver_number),
      color: driver ? `#${driver.team_colour}` : "#2cf5bf",
      driver_number: position.driver_number,
      point: displayTrackPoint(position, geometry),
      source: position.source,
      quality: position.quality,
      label: driverDotLabel(driver?.code ?? String(position.driver_number), position),
      isLeader: position.driver_number === leaderNumber
    };
  });
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

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min;
  return Math.max(min, Math.min(max, value));
}

function driverDotLabel(code: string, position: TrackPositionSample): string {
  const stale = position.stale_seconds == null ? "" : `, ${position.stale_seconds.toFixed(0)}s stale`;
  return `${code}: ${position.source}/${position.quality}${stale}`;
}
