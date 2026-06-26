import { For } from "solid-js";
import type { ReplaySnapshot, TrackGeometry } from "../../../shared/types/api";
import { closedRoadPath, pointsToPath, scalePoint } from "../lib/trackGeometry";
import {
  distanceMarkers,
  driverDots,
  hasRealTrackGeometry,
  startFinishLine,
  trackMapPlaceholder,
  trackMapRenderMode
} from "../lib/trackMapView";

export function TrackMapSvg(props: {
  snapshot: ReplaySnapshot;
  geometry?: TrackGeometry;
  geometryError?: unknown;
}) {
  const renderMode = () =>
    trackMapRenderMode(props.snapshot.track.map_mode, props.geometry, {
      error: props.geometryError
    });
  const dots = () =>
    renderMode() === "pending" || renderMode() === "error"
      ? []
      : driverDots(props.snapshot.track.positions, props.snapshot.timing.rows, props.geometry);

  return (
    <svg class="absolute inset-0 h-full w-full" viewBox="0 0 100 100" preserveAspectRatio="xMidYMid meet">
      {renderMode() === "real" && props.geometry ? (
        <RealGeometry geometry={props.geometry} />
      ) : renderMode() === "schematic" ? (
        <SchematicGeometry />
      ) : (
        <PendingGeometry mode={renderMode()} />
      )}
      <For each={dots()}>
        {(dot) => {
          const point = dot.point;
          return (
            <g>
              <title>{dot.label}</title>
              <ShowLeaderHalo show={dot.isLeader} x={point.x} y={point.y} />
              <circle
                cx={point.x}
                cy={point.y}
                r={dot.isLeader ? "2.8" : "2.3"}
                fill={dot.color}
                stroke="#f7fbff"
                stroke-width="0.5"
              />
              <text
                x={point.x + 2.8}
                y={point.y + 1.3}
                fill="#e7ecf0"
                font-size="3"
                font-family="monospace"
              >
                {dot.code}
              </text>
            </g>
          );
        }}
      </For>
    </svg>
  );
}

function PendingGeometry(props: { mode: "pending" | "error" | "real" | "schematic" }) {
  const placeholder = () => trackMapPlaceholder(props.mode);
  return (
    <>
      <rect x="8" y="12" width="84" height="76" fill="none" stroke="#2a333d" stroke-width="0.6" />
      <path d="M18 50 H82 M50 22 V78" stroke="#2a333d" stroke-dasharray="1.5 1.5" stroke-width="0.5" />
      <text x="50" y="49" text-anchor="middle" fill="#7b8794" font-size="3" font-family="monospace">
        {placeholder().label}
      </text>
      <text x="50" y="54" text-anchor="middle" fill="#56616d" font-size="2.3" font-family="monospace">
        {placeholder().detail}
      </text>
    </>
  );
}

function RealGeometry(props: { geometry: TrackGeometry }) {
  return (
    <>
      <path
        d={closedRoadPath(props.geometry.outer_edge, props.geometry.inner_edge, props.geometry.bounds)}
        fill="#20262c"
        stroke="none"
      />
      <path
        d={pointsToPath(props.geometry.outer_edge, props.geometry.bounds)}
        fill="none"
        stroke="#d4d7cc"
        stroke-width="0.9"
      />
      <path
        d={pointsToPath(props.geometry.inner_edge, props.geometry.bounds)}
        fill="none"
        stroke="#d4d7cc"
        stroke-width="0.9"
      />
      <path
        d={pointsToPath(props.geometry.centerline, props.geometry.bounds)}
        fill="none"
        stroke="#5b6571"
        stroke-dasharray="1.4 1.4"
        stroke-width="0.45"
      />
      <StartFinishMarker geometry={props.geometry} />
      <For each={distanceMarkers(props.geometry)}>
        {(marker) => {
          const point = scalePoint(marker.point, props.geometry.bounds);
          return (
            <g>
              <circle cx={point.x} cy={point.y} r="0.45" fill="#7b8794" />
              <text
                x={point.x + 1}
                y={point.y - 1}
                fill="#7b8794"
                font-size="2.1"
                font-family="monospace"
              >
                {marker.label}
              </text>
            </g>
          );
        }}
      </For>
    </>
  );
}

function StartFinishMarker(props: { geometry: TrackGeometry }) {
  const line = startFinishLine(props.geometry);
  if (!line) return null;
  return (
    <>
      <line
        x1={line.inner.x}
        y1={line.inner.y}
        x2={line.outer.x}
        y2={line.outer.y}
        stroke="#f7fbff"
        stroke-width="0.8"
      />
      <text
        x={line.start.x + 1.2}
        y={line.start.y - 1.2}
        fill="#9aa4af"
        font-size="2.4"
        font-family="monospace"
      >
        S/F
      </text>
    </>
  );
}

function SchematicGeometry() {
  return (
    <>
      <path
        d="M15 62 C22 22, 61 12, 82 27 C95 37, 81 66, 61 65 C42 64, 43 86, 23 79 C12 75, 9 70, 15 62"
        fill="none"
        stroke="#d4d7cc"
        stroke-width="1.6"
      />
      <path
        d="M18 62 C25 29, 60 20, 78 31 C88 39, 77 59, 61 58 C43 57, 45 76, 27 72 C18 70, 14 67, 18 62"
        fill="none"
        stroke="#5b6571"
        stroke-width="0.6"
      />
    </>
  );
}

function ShowLeaderHalo(props: { show: boolean; x: number; y: number }) {
  return props.show ? (
    <circle cx={props.x} cy={props.y} r="4.2" fill="none" stroke="#f5d547" stroke-width="0.55" />
  ) : null;
}
