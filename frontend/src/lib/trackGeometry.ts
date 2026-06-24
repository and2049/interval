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
