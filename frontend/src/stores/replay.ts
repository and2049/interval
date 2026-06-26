import { createEffect, createMemo, createResource, createSignal, onCleanup } from "solid-js";
import { api } from "../lib/api";
import {
  MVP_HISTORICAL_SESSION_KEY,
  readStoredSessionKey,
  writeStoredSessionKey
} from "../lib/sessionKeys";
import {
  activeReplayMetadata,
  activeReplaySnapshot,
  activeResourceError,
  activeResourceLoading,
  activeTrackGeometry,
  clampReplayTime,
  nextReplayTick,
  normalizeReplaySpeed,
  replayResourceSessionKey,
  snapshotRequest,
  shouldReloadSession
} from "../lib/replayPlayback";

export function createReplayStore() {
  const [sessionKey, setSessionKey] = createSignal(
    readStoredSessionKey() ?? MVP_HISTORICAL_SESSION_KEY
  );
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(1);
  const [time, setTime] = createSignal(0);

  const [metadata, { refetch: refetchMetadata }] = createResource(sessionKey, api.metadata);
  const snapshotSource = createMemo(
    () => snapshotRequest(sessionKey(), metadata(), time()),
    undefined,
    {
      equals: (previous, next) => previous?.key === next?.key && previous?.t === next?.t
    }
  );
  const [snapshot, { refetch }] = createResource(
    snapshotSource,
    ({ key, t }) => api.snapshot(key, t)
  );
  const [trackGeometry, { refetch: refetchTrackGeometry }] = createResource(
    () => replayResourceSessionKey(sessionKey(), metadata()),
    api.trackGeometry
  );
  const [events, { refetch: refetchEvents }] = createResource(
    () => replayResourceSessionKey(sessionKey(), metadata()),
    async (key) => ({ sessionKey: key, response: await api.events(key) })
  );
  const activeMetadata = () => activeReplayMetadata(sessionKey(), metadata());
  const activeSnapshot = () => activeReplaySnapshot(sessionKey(), snapshot());
  const activeGeometry = () => activeTrackGeometry(sessionKey(), trackGeometry());
  const activeGeometryError = () =>
    activeResourceError(
      sessionKey(),
      replayResourceSessionKey(sessionKey(), metadata()),
      trackGeometry.error
    );
  const activeEvents = () =>
    events()?.sessionKey === sessionKey() ? events()!.response.events : [];
  const activeEventsLoading = () =>
    activeResourceLoading(
      sessionKey(),
      replayResourceSessionKey(sessionKey(), metadata()),
      events.loading
    );
  const activeEventsError = () =>
    activeResourceError(
      sessionKey(),
      replayResourceSessionKey(sessionKey(), metadata()),
      events.error
    );

  let lastTick = performance.now();
  const timer = window.setInterval(() => {
    const now = performance.now();
    const elapsed = (now - lastTick) / 1000;
    lastTick = now;
    const meta = activeMetadata();
    const next = nextReplayTick({
      currentTime: time(),
      elapsedSeconds: elapsed,
      speed: speed(),
      maxT: meta?.max_t,
      playing: playing()
    });
    if (next.time !== time()) setTime(next.time);
    if (next.playing !== playing()) setPlaying(next.playing);
  }, 250);

  createEffect(() => {
    if (activeMetadata() && time() === 0) {
      void refetch();
    }
  });

  createEffect(() => {
    const session = activeMetadata()?.session;
    if (session) writeStoredSessionKey(session.session_key);
  });

  onCleanup(() => window.clearInterval(timer));

  return {
    sessionKey,
    metadata,
    activeMetadata,
    snapshot,
    activeSnapshot,
    trackGeometry,
    activeGeometry,
    activeGeometryError,
    events,
    activeEvents,
    activeEventsLoading,
    activeEventsError,
    time,
    playing,
    speed,
    setPlaying,
    setSpeed: (nextSpeed: number | ((current: number) => number)) => {
      setSpeed((current) =>
        normalizeReplaySpeed(typeof nextSpeed === "function" ? nextSpeed(current) : nextSpeed)
      );
    },
    seek: (nextTime: number) => {
      setTime(clampReplayTime(nextTime, activeMetadata()?.max_t ?? Number.POSITIVE_INFINITY));
    },
    openSession: (key: number) => {
      setPlaying(false);
      setTime(0);
      writeStoredSessionKey(key);
      if (shouldReloadSession(sessionKey(), key)) {
        void refetchMetadata();
        void refetch();
        void refetchTrackGeometry();
        void refetchEvents();
        return;
      }
      setSessionKey(key);
    }
  };
}
