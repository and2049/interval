import type { ReplaySnapshot, TrackGeometry } from "../../../shared/types/api";
import { mapModeClass, mapModeLabel } from "../lib/replayQuality";
import { Panel } from "./Panel";
import { TrackMapSvg } from "./TrackMapSvg";

export function TrackMap(props: {
  snapshot: ReplaySnapshot;
  geometry?: TrackGeometry;
  geometryError?: unknown;
}) {
  const displayMode = () => mapModeLabel(props.snapshot.track.map_mode) ?? "MAP UNKNOWN";

  return (
    <Panel title="Track Map" class="h-full min-h-0" testId="track-map">
      <div class="track-map relative h-full min-h-0 overflow-hidden">
        <TrackMapSvg
          snapshot={props.snapshot}
          geometry={props.geometry}
          geometryError={props.geometryError}
        />
        <div class="absolute left-3 top-3 grid grid-cols-3 gap-1 font-mono text-[0.68rem]">
          <span class="border border-line bg-panel px-2 py-1">L{props.snapshot.race_state.lap}</span>
          <span class="border border-line bg-panel px-2 py-1 uppercase">{props.snapshot.race_state.track_status}</span>
          <span class={`border bg-panel px-2 py-1 ${mapModeClass(props.snapshot.track.map_mode)}`}>{displayMode()}</span>
        </div>
      </div>
    </Panel>
  );
}
