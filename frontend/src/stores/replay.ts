import { createEffect, createResource, createSignal, onCleanup } from "solid-js";
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
import type { ReplaySnapshot } from "../../../shared/types/api";

const PLAYBACK_TICK_MS = 100;

export function createReplayStore() {
  const [sessionKey, setSessionKey] = createSignal(
    readStoredSessionKey() ?? MVP_HISTORICAL_SESSION_KEY
  );
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(1);
  const [time, setTime] = createSignal(0);
  const [streamStartTime, setStreamStartTime] = createSignal(0);
  const [currentSnapshot, setCurrentSnapshot] = createSignal<ReplaySnapshot>();
  const [snapshotLoading, setSnapshotLoading] = createSignal(false);
  const [snapshotError, setSnapshotError] = createSignal<unknown>();

  const [metadata, { refetch: refetchMetadata }] = createResource(sessionKey, api.metadata);
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
  let snapshotRequestId = 0;
  let stream: EventSource | undefined;

  async function loadSnapshot(t = time()) {
    const request = snapshotRequest(sessionKey(), metadata(), t);
    if (!request) return;
    const requestId = ++snapshotRequestId;
    setSnapshotLoading(true);
    setSnapshotError(undefined);
    try {
      const loaded = await api.snapshot(request.key, request.t);
      if (requestId === snapshotRequestId && loaded.cursor.session_key === sessionKey()) {
        setCurrentSnapshot(loaded);
      }
    } catch (error) {
      if (requestId === snapshotRequestId) setSnapshotError(error);
    } finally {
      if (requestId === snapshotRequestId) setSnapshotLoading(false);
    }
  }

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
  }, PLAYBACK_TICK_MS);

  createEffect(() => {
    const meta = activeMetadata();
    if (meta && !playing()) {
      void loadSnapshot(time());
    }
  });

  createEffect(() => {
    const session = activeMetadata()?.session;
    if (session) writeStoredSessionKey(session.session_key);
  });

  createEffect(() => {
    const meta = activeMetadata();
    if (!meta || !playing()) {
      stream?.close();
      stream = undefined;
      return;
    }

    stream?.close();
    const source = new EventSource(
      api.streamUrl(meta.session.session_key, streamStartTime(), speed())
    );
    stream = source;
    setSnapshotLoading(true);
    setSnapshotError(undefined);

    source.addEventListener("snapshot", (event) => {
      try {
        const snapshot = JSON.parse((event as MessageEvent).data) as ReplaySnapshot;
        if (snapshot.cursor.session_key !== sessionKey()) return;
        setCurrentSnapshot(snapshot);
        setSnapshotLoading(false);
      } catch (error) {
        setSnapshotError(error);
      }
    });
    source.addEventListener("error", () => {
      if (source.readyState === EventSource.CLOSED) {
        setSnapshotError(new Error("Replay stream disconnected."));
      }
    });
    source.addEventListener("end", () => {
      setPlaying(false);
      source.close();
      if (stream === source) stream = undefined;
    });
  });

  onCleanup(() => {
    window.clearInterval(timer);
    stream?.close();
  });

  const snapshot = Object.assign(() => currentSnapshot(), {
    get loading() {
      return snapshotLoading();
    },
    get error() {
      return snapshotError();
    }
  });

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
    setPlaying: (nextPlaying: boolean | ((current: boolean) => boolean)) => {
      const resolved =
        typeof nextPlaying === "function" ? nextPlaying(playing()) : nextPlaying;
      if (resolved) setStreamStartTime(time());
      setPlaying(resolved);
    },
    setSpeed: (nextSpeed: number | ((current: number) => number)) => {
      const normalized = normalizeReplaySpeed(
        typeof nextSpeed === "function" ? nextSpeed(speed()) : nextSpeed
      );
      setSpeed(normalized);
      setStreamStartTime(time());
    },
    seek: (nextTime: number) => {
      const clamped = clampReplayTime(
        nextTime,
        activeMetadata()?.max_t ?? Number.POSITIVE_INFINITY
      );
      setTime(clamped);
      setStreamStartTime(clamped);
      void loadSnapshot(clamped);
    },
    openSession: (key: number) => {
      setPlaying(false);
      setTime(0);
      setStreamStartTime(0);
      setCurrentSnapshot(undefined);
      setSnapshotError(undefined);
      writeStoredSessionKey(key);
      if (shouldReloadSession(sessionKey(), key)) {
        void refetchMetadata();
        void loadSnapshot(0);
        void refetchTrackGeometry();
        void refetchEvents();
        return;
      }
      setSessionKey(key);
    }
  };
}
