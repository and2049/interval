import type { TrackBounds, TrackPoint } from "../../../shared/types/api";

const VIEWBOX_MIN = 5;
const VIEWBOX_MAX = 95;
const VIEWBOX_SIZE = VIEWBOX_MAX - VIEWBOX_MIN;

export const hasUsableGeometry = (points?: TrackPoint[]) => Boolean(points && points.length > 1);

export interface TrackPointLookup {
  points: TrackPoint[];
  relativeDistances: number[];
}

export const createTrackPointLookup = (points: TrackPoint[]): TrackPointLookup | undefined => {
  if (!hasUsableGeometry(points)) return undefined;
  return {
    points,
    relativeDistances: points.map((point) => point.relative_distance)
  };
};

export const scalePoint = (point: { x: number; y: number }, bounds: TrackBounds) => {
  const width = Math.max(1, bounds.max_x - bounds.min_x);
  const height = Math.max(1, bounds.max_y - bounds.min_y);
  const scale = Math.min(VIEWBOX_SIZE / width, VIEWBOX_SIZE / height);
  const drawnWidth = width * scale;
  const drawnHeight = height * scale;
  const offsetX = VIEWBOX_MIN + (VIEWBOX_SIZE - drawnWidth) / 2;
  const offsetY = VIEWBOX_MIN + (VIEWBOX_SIZE - drawnHeight) / 2;

  return {
    x: offsetX + (point.x - bounds.min_x) * scale,
    y: offsetY + (bounds.max_y - point.y) * scale
  };
};

export const pointsToPath = (points: TrackPoint[], bounds: TrackBounds) =>
  points
    .map((point, index) => {
      const scaled = scalePoint(point, bounds);
      return `${index === 0 ? "M" : "L"} ${scaled.x.toFixed(2)} ${scaled.y.toFixed(2)}`;
    })
    .join(" ");

export const closedRoadPath = (
  outerEdge: TrackPoint[],
  innerEdge: TrackPoint[],
  bounds: TrackBounds
) => {
  if (!hasUsableGeometry(outerEdge) || !hasUsableGeometry(innerEdge)) return "";
  const outer = outerEdge
    .map((point, index) => {
      const scaled = scalePoint(point, bounds);
      return `${index === 0 ? "M" : "L"} ${scaled.x.toFixed(2)} ${scaled.y.toFixed(2)}`;
    })
    .join(" ");
  const inner = [...innerEdge]
    .reverse()
    .map((point) => {
      const scaled = scalePoint(point, bounds);
      return `L ${scaled.x.toFixed(2)} ${scaled.y.toFixed(2)}`;
    })
    .join(" ");
  return `${outer} ${inner} Z`;
};

export const pointAtRelativeDistance = (points: TrackPoint[], relativeDistance: number) => {
  const lookup = createTrackPointLookup(points);
  return lookup ? pointAtRelativeDistanceLookup(lookup, relativeDistance) : undefined;
};

export const pointAtRelativeDistanceLookup = (
  lookup: TrackPointLookup,
  relativeDistance: number
) => {
  const points = lookup.points;
  if (!hasUsableGeometry(points)) return undefined;
  const relative = ((relativeDistance % 1) + 1) % 1;
  const index = segmentIndexForRelativeDistance(lookup.relativeDistances, relative);
  const current = points[index];
  const next = points[index + 1];
  if (current && next) {
    const span = Math.max(0.000001, next.relative_distance - current.relative_distance);
    const ratio = Math.max(0, Math.min(1, (relative - current.relative_distance) / span));
    return {
      x: current.x + (next.x - current.x) * ratio,
      y: current.y + (next.y - current.y) * ratio
    };
  }
  return points[0];
};

function segmentIndexForRelativeDistance(relativeDistances: number[], relative: number) {
  let low = 0;
  let high = relativeDistances.length - 2;
  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    const current = relativeDistances[mid];
    const next = relativeDistances[mid + 1];
    if (relative >= current && relative <= next) return mid;
    if (relative < current) {
      high = mid - 1;
    } else {
      low = mid + 1;
    }
  }
  return 0;
}
