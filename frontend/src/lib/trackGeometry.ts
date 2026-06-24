import type { TrackBounds, TrackPoint } from "../../../shared/types/api";

const VIEWBOX_MIN = 5;
const VIEWBOX_MAX = 95;
const VIEWBOX_SIZE = VIEWBOX_MAX - VIEWBOX_MIN;

export const hasUsableGeometry = (points?: TrackPoint[]) => Boolean(points && points.length > 1);

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
  if (!hasUsableGeometry(points)) return undefined;
  const relative = ((relativeDistance % 1) + 1) % 1;
  for (let index = 0; index < points.length - 1; index += 1) {
    const current = points[index];
    const next = points[index + 1];
    if (relative >= current.relative_distance && relative <= next.relative_distance) {
      const span = Math.max(0.000001, next.relative_distance - current.relative_distance);
      const ratio = Math.max(0, Math.min(1, (relative - current.relative_distance) / span));
      return {
        x: current.x + (next.x - current.x) * ratio,
        y: current.y + (next.y - current.y) * ratio
      };
    }
  }
  return points[0];
};
