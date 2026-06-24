import { createEffect, createResource, createSignal, onCleanup } from "solid-js";
import { api } from "../lib/api";

const DEMO_SESSION_KEY = 9839;

export function createReplayStore() {
  const [sessionKey, setSessionKey] = createSignal(DEMO_SESSION_KEY);
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(1);
  const [time, setTime] = createSignal(0);

  const [metadata, { refetch: refetchMetadata }] = createResource(sessionKey, api.metadata);
  const [snapshot, { refetch }] = createResource(
    () => ({ key: sessionKey(), t: time() }),
    ({ key, t }) => api.snapshot(key, t)
  );
  const [trackGeometry, { refetch: refetchTrackGeometry }] = createResource(
    sessionKey,
    api.trackGeometry
  );

  let lastTick = performance.now();
  const timer = window.setInterval(() => {
    const now = performance.now();
    const elapsed = (now - lastTick) / 1000;
    lastTick = now;
    const meta = metadata();
    if (!playing() || !meta) return;
    setTime((current) => Math.min(meta.max_t, current + elapsed * speed()));
  }, 250);

  createEffect(() => {
    if (metadata() && time() === 0) {
      void refetch();
    }
  });

  createEffect(() => {
    if (metadata() && time() >= metadata()!.max_t) {
      setPlaying(false);
    }
  });

  onCleanup(() => window.clearInterval(timer));

  return {
    sessionKey,
    metadata,
    snapshot,
    trackGeometry,
    time,
    playing,
    speed,
    setPlaying,
    setSpeed,
    seek: setTime,
    openSession: (key: number) => {
      setPlaying(false);
      setTime(0);
      if (key === sessionKey()) {
        void refetchMetadata();
        void refetch();
        void refetchTrackGeometry();
        return;
      }
      setSessionKey(key);
    }
  };
}
