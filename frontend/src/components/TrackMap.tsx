import { For } from "solid-js";
import type { ReplaySnapshot, TrackGeometry } from "../../../shared/types/api";
import {
  closedRoadPath,
  hasUsableGeometry,
  pointAtRelativeDistance,
  pointsToPath,
  scalePoint
} from "../lib/trackGeometry";
import { Panel } from "./Panel";

export function TrackMap(props: { snapshot: ReplaySnapshot; geometry?: TrackGeometry }) {
  const driverByNumber = (number: number) =>
    props.snapshot.timing.rows.find((row) => row.driver.driver_number === number)?.driver;
  const realGeometry = () =>
    props.geometry?.quality === "ready" && hasUsableGeometry(props.geometry.centerline);
  const displayMode = () => props.snapshot.track.map_mode.toUpperCase();
  const leaderNumber = () => props.snapshot.timing.rows[0]?.driver.driver_number;
  const positionPoint = (position: { x: number; y: number }) =>
    realGeometry() && props.geometry
      ? scalePoint(position, props.geometry.bounds)
      : { x: Math.max(5, Math.min(95, position.x)), y: Math.max(8, Math.min(92, position.y)) };

  return (
    <Panel title="Track Map" class="h-full">
      <div class="track-map relative h-[31rem] overflow-hidden">
        <svg class="absolute inset-0 h-full w-full" viewBox="0 0 100 100" preserveAspectRatio="xMidYMid meet">
          {realGeometry() && props.geometry ? (
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
              {(() => {
                const start = scalePoint(props.geometry.centerline[0], props.geometry.bounds);
                const inner = scalePoint(props.geometry.inner_edge[0], props.geometry.bounds);
                const outer = scalePoint(props.geometry.outer_edge[0], props.geometry.bounds);
                return (
                  <>
                    <line
                      x1={inner.x}
                      y1={inner.y}
                      x2={outer.x}
                      y2={outer.y}
                      stroke="#f7fbff"
                      stroke-width="0.8"
                    />
                    <text
                      x={start.x + 1.2}
                      y={start.y - 1.2}
                      fill="#9aa4af"
                      font-size="2.4"
                      font-family="monospace"
                    >
                      S/F
                    </text>
                  </>
                );
              })()}
              <For each={distanceMarkers(props.geometry)}>
                {(marker) => {
                  const point = scalePoint(marker.point, props.geometry!.bounds);
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
          ) : (
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
          )}
          <For each={props.snapshot.track.positions}>
            {(position) => {
              const driver = driverByNumber(position.driver_number);
              const point = positionPoint(position);
              const isLeader = position.driver_number === leaderNumber();
              return (
                <g>
                  <ShowLeaderHalo show={isLeader} x={point.x} y={point.y} />
                  <circle
                    cx={point.x}
                    cy={point.y}
                    r={isLeader ? "2.8" : "2.3"}
                    fill={driver ? `#${driver.team_colour}` : "#2cf5bf"}
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
                    {driver?.code ?? position.driver_number}
                  </text>
                </g>
              );
            }}
          </For>
        </svg>
        <div class="absolute left-3 top-3 grid grid-cols-3 gap-1 font-mono text-[0.68rem]">
          <span class="border border-line bg-panel px-2 py-1">L{props.snapshot.race_state.lap}</span>
          <span class="border border-line bg-panel px-2 py-1 uppercase">{props.snapshot.race_state.track_status}</span>
          <span class={`border bg-panel px-2 py-1 ${modeClass(props.snapshot.track.map_mode)}`}>{displayMode()}</span>
        </div>
      </div>
    </Panel>
  );
}

function ShowLeaderHalo(props: { show: boolean; x: number; y: number }) {
  return props.show ? (
    <circle cx={props.x} cy={props.y} r="4.2" fill="none" stroke="#f5d547" stroke-width="0.55" />
  ) : null;
}

function distanceMarkers(geometry: TrackGeometry) {
  const length = geometry.circuit_length ?? 0;
  if (length <= 0) return [];
  const markers = [];
  const count = Math.min(6, Math.floor(length / 1000));
  for (let index = 1; index <= count; index += 1) {
    const relative = (index * 1000) / length;
    const point = pointAtRelativeDistance(geometry.centerline, relative);
    if (point) markers.push({ label: `${index}K`, point });
  }
  return markers;
}

function modeClass(mode: ReplaySnapshot["track"]["map_mode"]) {
  switch (mode) {
    case "gps":
      return "border-mint text-mint";
    case "projected":
      return "border-amber text-amber";
    default:
      return "border-line text-slate-400";
  }
}
