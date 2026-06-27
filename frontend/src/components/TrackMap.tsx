import { createEffect, createMemo, createSignal, onCleanup, untrack } from "solid-js";
import type { ReplaySnapshot, TrackGeometry } from "../../../shared/types/api";
import { hasRealTrackGeometry, interpolateTrackPositions } from "../lib/trackMapView";
import { mapModeClass, mapModeLabel } from "../lib/replayQuality";
import { createTrackPointLookup } from "../lib/trackGeometry";
import { Panel } from "./Panel";
import { TrackMapSvg } from "./TrackMapSvg";

const MIN_TRACK_TRANSITION_MS = 80;
const MAX_TRACK_TRANSITION_MS = 1_200;

export function TrackMap(props: {
  snapshot: ReplaySnapshot;
  geometry?: TrackGeometry;
  geometryError?: unknown;
  frameStepSeconds: number;
  playing: boolean;
  speed: number;
}) {
  const displayMode = () => mapModeLabel(props.snapshot.track.map_mode) ?? "MAP UNKNOWN";
  const [previousSnapshot, setPreviousSnapshot] = createSignal<ReplaySnapshot>();
  const [animationProgress, setAnimationProgress] = createSignal(1);
  let lastSnapshot: ReplaySnapshot | undefined;
  let animationFrame: number | undefined;

  const cancelAnimation = () => {
    if (animationFrame !== undefined) {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = undefined;
    }
  };

  createEffect(() => {
    const next = props.snapshot;
    const previous = lastSnapshot;
    lastSnapshot = next;
    cancelAnimation();

    const options = untrack(() => ({
      playing: props.playing,
      frameStepSeconds: props.frameStepSeconds,
      speed: props.speed
    }));

    if (!canAnimateSnapshotTransition(previous, next, options.playing)) {
      setPreviousSnapshot(undefined);
      setAnimationProgress(1);
      return;
    }

    setPreviousSnapshot(previous);
    setAnimationProgress(0);
    const duration = trackTransitionDurationMs(options.frameStepSeconds, options.speed);
    const startedAt = performance.now();

    const tick = (now: number) => {
      const progress = Math.min(1, (now - startedAt) / duration);
      setAnimationProgress(progress);
      if (progress < 1) {
        animationFrame = window.requestAnimationFrame(tick);
      } else {
        animationFrame = undefined;
      }
    };

    animationFrame = window.requestAnimationFrame(tick);
  });

  createEffect(() => {
    if (!props.playing) {
      cancelAnimation();
      setPreviousSnapshot(undefined);
      setAnimationProgress(1);
    }
  });

  onCleanup(cancelAnimation);

  const centerlineLookup = createMemo(() =>
    hasRealTrackGeometry(props.geometry)
      ? createTrackPointLookup(props.geometry.centerline)
      : undefined
  );
  const positions = createMemo(() =>
    interpolateTrackPositions(
      previousSnapshot()?.track.positions,
      props.snapshot.track.positions,
      animationProgress(),
      props.geometry,
      centerlineLookup()
    )
  );

  return (
    <Panel title="Track Map" class="h-full min-h-0" testId="track-map">
      <div class="track-map relative h-full min-h-0 overflow-hidden">
        <TrackMapSvg
          snapshot={props.snapshot}
          geometry={props.geometry}
          geometryError={props.geometryError}
          positions={positions()}
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

function canAnimateSnapshotTransition(
  previous: ReplaySnapshot | undefined,
  next: ReplaySnapshot,
  playing: boolean
) {
  return Boolean(
    playing &&
      previous &&
      previous.cursor.session_key === next.cursor.session_key &&
      previous.cursor.t < next.cursor.t &&
      previous.track.positions.length > 0 &&
      next.track.positions.length > 0
  );
}

function trackTransitionDurationMs(frameStepSeconds: number, speed: number) {
  const normalizedSpeed = Number.isFinite(speed) && speed > 0 ? speed : 1;
  const frameMs = Number.isFinite(frameStepSeconds) && frameStepSeconds > 0
    ? (frameStepSeconds * 1000) / normalizedSpeed
    : MAX_TRACK_TRANSITION_MS;
  return Math.max(MIN_TRACK_TRANSITION_MS, Math.min(MAX_TRACK_TRANSITION_MS, frameMs));
}
