import { createEffect, createResource, createSignal, onCleanup } from "solid-js";
import { api } from "../lib/api";
import { clearStoredSessionKey, readStoredSessionKey, writeStoredSessionKey } from "../lib/sessionKeys";
import {
  activeReplayMetadata,
  activeReplaySnapshot,
  activeResourceError,
  activeResourceLoading,
  activeTrackGeometry,
  clampReplayTime,
  liveAvailabilityAfterStartError,
  liveAvailabilityPollDelayMs,
  liveCheckErrorMessage,
  liveCurrentMessage,
  liveStartErrorMessage,
  nextReplayTick,
  liveSimulationSessionKeyToStop,
  openF1LiveSessionKeyToStop,
  normalizeReplaySpeed,
  replayResourceSessionKey,
  serverSentErrorMessage,
  sessionKeyAfterLiveStops,
  shouldApplyLiveResourceResult,
  shouldApplyLiveStartResult,
  shouldClearMissingHistoricalReplay,
  shouldHideHistoricalResourceError,
  shouldPollLiveAvailability,
  snapshotRequest,
  shouldReloadSession
} from "../lib/replayPlayback";
import type {
  LiveAvailability,
  LiveSessionStatus,
  ReplayEvent,
  ReplayMetadata,
  ReplaySnapshot,
  TrackGeometry
} from "../../../shared/types/api";

const PLAYBACK_TICK_MS = 100;
const LIVE_AVAILABILITY_POLL_TICK_MS = 1_000;
type LiveSimulationConnection = "idle" | "connecting" | "connected" | "disconnected";
type LiveConnection = "idle" | "connecting" | "connected" | "reconnecting" | "disconnected";

export function createReplayStore() {
  const [sessionKey, setSessionKey] = createSignal<number | undefined>(readStoredSessionKey());
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(1);
  const [time, setTime] = createSignal(0);
  const [streamStartTime, setStreamStartTime] = createSignal(0);
  const [currentSnapshot, setCurrentSnapshot] = createSignal<ReplaySnapshot>();
  const [liveMetadata, setLiveMetadata] = createSignal<ReplayMetadata>();
  const [liveGeometry, setLiveGeometry] = createSignal<TrackGeometry>();
  const [liveStatus, setLiveStatus] = createSignal<LiveSessionStatus>();
  const [liveEvents, setLiveEvents] = createSignal<ReplayEvent[]>([]);
  const [liveAvailability, setLiveAvailability] = createSignal<LiveAvailability>("inactive");
  const [liveAvailabilityMessage, setLiveAvailabilityMessage] = createSignal<string>();
  const [liveAvailabilityChecking, setLiveAvailabilityChecking] = createSignal(false);
  const [liveActive, setLiveActive] = createSignal(false);
  const [liveConnection, setLiveConnection] = createSignal<LiveConnection>("idle");
  const [liveReconnectNonce, setLiveReconnectNonce] = createSignal(0);
  const [liveSimulationActive, setLiveSimulationActive] = createSignal(false);
  const [liveSimulationConnection, setLiveSimulationConnection] =
    createSignal<LiveSimulationConnection>("idle");
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
  const displayMetadata = () => {
    const live = liveMetadata();
    return (liveActive() || liveSimulationActive()) && live?.session.session_key === sessionKey()
      ? live
      : activeMetadata();
  };
  const activeSnapshot = () => activeReplaySnapshot(sessionKey(), snapshot());
  const activeGeometry = () =>
    liveActive() && liveGeometry()?.session_key === sessionKey()
      ? liveGeometry()
      : activeTrackGeometry(sessionKey(), trackGeometry());
  const activeGeometryError = () =>
    shouldHideHistoricalResourceError(liveActive())
      ? undefined
      : activeResourceError(
          sessionKey(),
          replayResourceSessionKey(sessionKey(), metadata()),
          trackGeometry.error
        );
  const activeEvents = () =>
    liveActive()
      ? liveEvents()
      : events()?.sessionKey === sessionKey()
        ? events()!.response.events
        : [];
  const activeEventsLoading = () =>
    liveActive()
      ? false
      : activeResourceLoading(
          sessionKey(),
          replayResourceSessionKey(sessionKey(), metadata()),
          events.loading
        );
  const activeEventsError = () =>
    liveActive()
      ? undefined
      : activeResourceError(
          sessionKey(),
          replayResourceSessionKey(sessionKey(), metadata()),
          events.error
        );

  let lastTick = performance.now();
  let snapshotRequestId = 0;
  let replayStream: EventSource | undefined;
  let openF1LiveStream: EventSource | undefined;
  let liveStream: EventSource | undefined;
  let initializedSessionKey: number | undefined;
  let checkedLiveOnStartup = false;
  let lastLiveStatusRefresh = 0;
  let openF1LiveReconnectTimer: number | undefined;
  let openF1LiveReconnectAttempts = 0;
  let openF1LiveCheckRequestId = 0;
  let openF1LiveStartRequestId = 0;
  let lastLiveAvailabilityCheck = 0;
  let returnSessionKeyAfterLive: number | undefined;

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
      playing: playing() && !liveSimulationActive()
        && !liveActive()
    });
    if (next.time !== time()) setTime(next.time);
    if (next.playing !== playing()) setPlaying(next.playing);
  }, PLAYBACK_TICK_MS);
  const liveAvailabilityTimer = window.setInterval(() => {
    const now = performance.now();
    const pollDelay = liveAvailabilityPollDelayMs(liveAvailability(), liveAvailabilityMessage());
    if (
      now - lastLiveAvailabilityCheck >= pollDelay &&
      shouldPollLiveAvailability(liveAvailability()) &&
      !liveAvailabilityChecking() &&
      !liveActive() &&
      !liveSimulationActive()
    ) {
      void checkOpenF1Live("poll");
    }
  }, LIVE_AVAILABILITY_POLL_TICK_MS);

  createEffect(() => {
    const meta = activeMetadata();
    if (meta && !playing() && !liveSimulationActive() && !liveActive()) {
      void loadSnapshot(time());
    }
  });

  createEffect(() => {
    if (checkedLiveOnStartup) return;
    checkedLiveOnStartup = true;
    void checkOpenF1Live();
  });

  createEffect(() => {
    if (
      shouldClearMissingHistoricalReplay({
        metadataError: metadata.error,
        metadataLoading: metadata.loading,
        sessionKey: sessionKey(),
        liveActive: liveActive(),
        liveSimulationActive: liveSimulationActive()
      })
    ) {
      clearStoredSessionKey();
      setSessionKey(undefined);
    }
  });

  createEffect(() => {
    const session = activeMetadata()?.session;
    if (session) writeStoredSessionKey(session.session_key);
  });

  createEffect(() => {
    const meta = activeMetadata();
    if (!meta || playing() || liveSimulationActive() || liveActive()) return;
    if (initializedSessionKey === meta.session.session_key) return;
    initializedSessionKey = meta.session.session_key;
    const startT = meta.race_start_t > 0 ? meta.race_start_t : meta.min_t;
    setTime(startT);
    setStreamStartTime(startT);
    void loadSnapshot(startT);
  });

  createEffect(() => {
    const meta = activeMetadata();
    if (!meta || !playing() || liveSimulationActive() || liveActive()) {
      replayStream?.close();
      replayStream = undefined;
      return;
    }

    replayStream?.close();
    const source = new EventSource(
      api.streamUrl(meta.session.session_key, streamStartTime(), speed())
    );
    replayStream = source;
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
      if (replayStream === source) replayStream = undefined;
    });
  });

  createEffect(() => {
    const key = sessionKey();
    if (!key || !liveSimulationActive()) {
      liveStream?.close();
      liveStream = undefined;
      return;
    }

    liveStream?.close();
    const source = new EventSource(api.liveSimulationStreamUrl(key));
    liveStream = source;
    setLiveSimulationConnection("connecting");
    setSnapshotLoading(true);
    setSnapshotError(undefined);

    source.addEventListener("metadata", (event) => {
      try {
        setLiveMetadata(JSON.parse((event as MessageEvent).data));
      } catch (error) {
        setSnapshotError(error);
      }
    });
    source.addEventListener("snapshot", (event) => {
      try {
        const snapshot = JSON.parse((event as MessageEvent).data) as ReplaySnapshot;
        if (snapshot.cursor.session_key !== sessionKey()) return;
        setCurrentSnapshot(snapshot);
        setTime(snapshot.cursor.t);
        setLiveSimulationConnection("connected");
        setSnapshotLoading(false);
      } catch (error) {
        setSnapshotError(error);
        setLiveSimulationConnection("disconnected");
      }
    });
    source.addEventListener("error", () => {
      if (source.readyState === EventSource.CLOSED) {
        setSnapshotError(new Error("Live simulation stream disconnected."));
        setLiveSimulationConnection("disconnected");
      }
    });
    source.addEventListener("end", () => {
      setLiveSimulationActive(false);
      setLiveSimulationConnection("idle");
      clearLiveRuntimeResources();
      source.close();
      if (liveStream === source) liveStream = undefined;
    });
  });

  createEffect(() => {
    const key = sessionKey();
    liveReconnectNonce();
    if (!key || !liveActive()) {
      openF1LiveStream?.close();
      openF1LiveStream = undefined;
      clearOpenF1LiveReconnect();
      return;
    }

    openF1LiveStream?.close();
    const source = new EventSource(api.liveStreamUrl(key));
    openF1LiveStream = source;
    setLiveConnection("connecting");
    setSnapshotLoading(true);
    setSnapshotError(undefined);

    source.addEventListener("metadata", (event) => {
      try {
        setLiveMetadata(JSON.parse((event as MessageEvent).data));
      } catch (error) {
        setSnapshotError(error);
      }
    });
    source.addEventListener("snapshot", (event) => {
      try {
        const snapshot = JSON.parse((event as MessageEvent).data) as ReplaySnapshot;
        if (snapshot.cursor.session_key !== sessionKey()) return;
        setCurrentSnapshot(snapshot);
        setTime(snapshot.cursor.t);
        setLiveConnection("connected");
        setSnapshotLoading(false);
        openF1LiveReconnectAttempts = 0;
        void refreshOpenF1LiveStatus(snapshot.cursor.session_key);
      } catch (error) {
        setSnapshotError(error);
        setLiveConnection("disconnected");
      }
    });
    source.addEventListener("event", (event) => {
      try {
        const replayEvent = JSON.parse((event as MessageEvent).data) as ReplayEvent;
        if (replayEvent.t <= time()) {
          setLiveEvents((current) =>
            current.some((existing) => existing.id === replayEvent.id)
              ? current
              : [...current, replayEvent].sort((a, b) => a.t - b.t)
          );
        }
      } catch (error) {
        setSnapshotError(error);
      }
    });
    source.addEventListener("error", (event) => {
      if (!liveActive()) return;
      const serverMessage = serverSentErrorMessage(event);
      if (serverMessage) {
        setSnapshotError(new Error(serverMessage));
        setSnapshotLoading(false);
        void refreshOpenF1LiveStatus(key);
        source.close();
        if (openF1LiveStream === source) openF1LiveStream = undefined;
        scheduleOpenF1LiveReconnect(serverMessage);
        return;
      }
      source.close();
      if (openF1LiveStream === source) openF1LiveStream = undefined;
      scheduleOpenF1LiveReconnect();
    });
    source.addEventListener("end", () => {
      const returnKey = sessionKeyAfterLiveStops(sessionKey(), returnSessionKeyAfterLive);
      setLiveActive(false);
      setLiveAvailability("inactive");
      setLiveConnection("idle");
      clearOpenF1LiveReconnect();
      clearLiveRuntimeResources();
      setSessionKey(returnKey);
      returnSessionKeyAfterLive = undefined;
      setLiveAvailabilityMessage("Live session ended.");
      source.close();
      if (openF1LiveStream === source) openF1LiveStream = undefined;
    });
  });

  onCleanup(() => {
    window.clearInterval(timer);
    window.clearInterval(liveAvailabilityTimer);
    clearOpenF1LiveReconnect();
    replayStream?.close();
    openF1LiveStream?.close();
    liveStream?.close();
  });

  async function startOpenF1Live(key: number): Promise<boolean> {
    const requestId = ++openF1LiveStartRequestId;
    setPlaying(false);
    if (liveSimulationActive()) {
      const simulationKey = sessionKey();
      if (simulationKey != null) await api.liveSimulationStop(simulationKey).catch(() => undefined);
    }
    setLiveSimulationActive(false);
    setLiveSimulationConnection("idle");
    liveStream?.close();
    liveStream = undefined;
    clearOpenF1LiveReconnect();
    setLiveEvents([]);
    setSnapshotError(undefined);
    setSnapshotLoading(true);
    setLiveConnection("connecting");
    returnSessionKeyAfterLive = activeMetadata()?.session.session_key;
    try {
      await api.liveStart(key);
      const status = await api.liveStatus(key).catch(() => undefined);
      const metadata = await api.liveMetadata(key);
      const snapshot = await api.liveSnapshot(key);
      const geometry = await api.liveTrackGeometry(key).catch(() => undefined);
      const events = await api.liveEvents(key).catch(() => undefined);
      if (!shouldApplyLiveStartResult(requestId, openF1LiveStartRequestId)) return false;
      setSessionKey(key);
      if (status?.session_key === key) setLiveStatus(status);
      if (metadata.session.session_key === key) setLiveMetadata(metadata);
      if (snapshot.cursor.session_key === key) {
        setCurrentSnapshot(snapshot);
        setTime(snapshot.cursor.t);
      }
      if (geometry?.session_key === key) setLiveGeometry(geometry);
      if (events) setLiveEvents(events.events);
      setLiveActive(true);
      setLiveConnection("connected");
      return true;
    } catch (error) {
      if (!shouldApplyLiveStartResult(requestId, openF1LiveStartRequestId)) return false;
      setSnapshotError(error);
      setLiveAvailability(liveAvailabilityAfterStartError(error));
      setLiveAvailabilityMessage(liveStartErrorMessage(error));
      setLiveConnection("disconnected");
      setLiveActive(false);
      clearOpenF1LiveResources();
      returnSessionKeyAfterLive = undefined;
      return false;
    } finally {
      if (shouldApplyLiveStartResult(requestId, openF1LiveStartRequestId)) {
        setSnapshotLoading(false);
      }
    }
  }

  async function checkOpenF1Live(source: "startup" | "manual" | "poll" = "manual") {
    const requestId = ++openF1LiveCheckRequestId;
    lastLiveAvailabilityCheck = performance.now();
    setLiveAvailabilityChecking(true);
    if (source !== "poll") setLiveAvailabilityMessage("Checking live race status...");
    try {
      const current = await api.liveCurrent();
      if (!shouldApplyLiveStartResult(requestId, openF1LiveCheckRequestId)) return;
      setLiveAvailability(current.availability);
      const key = current.session?.session_key;
      if (!current.active || !key) {
        setLiveAvailabilityMessage(
          liveCurrentMessage(
            current.availability,
            current.message,
            current.next_session,
            current.next_meeting
          )
        );
        return;
      }
      setLiveAvailabilityMessage("Opening active OpenF1 live session...");
      if (await startOpenF1Live(key)) {
        setLiveAvailabilityMessage(undefined);
      }
    } catch (error) {
      if (!shouldApplyLiveStartResult(requestId, openF1LiveCheckRequestId)) return;
      setLiveAvailability("error");
      setLiveAvailabilityMessage(liveCheckErrorMessage(error));
    } finally {
      if (shouldApplyLiveStartResult(requestId, openF1LiveCheckRequestId)) {
        setLiveAvailabilityChecking(false);
      }
    }
  }

  async function refreshOpenF1LiveStatus(key: number) {
    const now = performance.now();
    if (now - lastLiveStatusRefresh < 2_000) return;
    lastLiveStatusRefresh = now;
    const status = await api.liveStatus(key).catch(() => undefined);
    if (shouldApplyLiveResourceResult(status?.session_key, sessionKey(), liveActive())) {
      setLiveStatus(status);
    }
    const geometry = await api.liveTrackGeometry(key).catch(() => undefined);
    if (shouldApplyLiveResourceResult(geometry?.session_key, sessionKey(), liveActive())) {
      setLiveGeometry(geometry);
    }
  }

  function scheduleOpenF1LiveReconnect(reason?: string) {
    clearOpenF1LiveReconnect();
    openF1LiveReconnectAttempts += 1;
    const delayMs = Math.min(15_000, 1_000 * 2 ** Math.min(openF1LiveReconnectAttempts - 1, 4));
    setLiveConnection("reconnecting");
    const prefix = reason?.trim() || "OpenF1 live stream disconnected.";
    setSnapshotError(new Error(`${prefix} Reconnecting in ${Math.round(delayMs / 1000)}s.`));
    openF1LiveReconnectTimer = window.setTimeout(() => {
      openF1LiveReconnectTimer = undefined;
      if (liveActive()) setLiveReconnectNonce((value) => value + 1);
    }, delayMs);
  }

  function clearOpenF1LiveReconnect() {
    if (openF1LiveReconnectTimer !== undefined) {
      window.clearTimeout(openF1LiveReconnectTimer);
      openF1LiveReconnectTimer = undefined;
    }
  }

  function stopAbandonedOpenF1LiveSession() {
    const key = openF1LiveSessionKeyToStop(sessionKey(), liveActive());
    if (key != null) void api.liveStop(key).catch(() => undefined);
  }

  function stopAbandonedLiveRuntimes() {
    stopAbandonedOpenF1LiveSession();
    const simulationKey = liveSimulationSessionKeyToStop(sessionKey(), liveSimulationActive());
    if (simulationKey != null) void api.liveSimulationStop(simulationKey).catch(() => undefined);
  }

  function clearOpenF1LiveResources() {
    setLiveMetadata(undefined);
    setLiveGeometry(undefined);
    setLiveStatus(undefined);
    setLiveEvents([]);
  }

  function clearLiveRuntimeResources() {
    clearOpenF1LiveResources();
    setCurrentSnapshot(undefined);
  }

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
    displayMetadata,
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
    liveSimulationActive,
    liveSimulationConnection,
    liveActive,
    liveConnection,
    liveStatus,
    liveAvailability,
    liveAvailabilityChecking,
    liveAvailabilityMessage,
    liveChannels: () => liveStatus()?.channels ?? [],
    speed,
    setPlaying: (nextPlaying: boolean | ((current: boolean) => boolean)) => {
      if (liveSimulationActive() || liveActive()) return;
      const resolved =
        typeof nextPlaying === "function" ? nextPlaying(playing()) : nextPlaying;
      if (resolved) setStreamStartTime(time());
      setPlaying(resolved);
    },
    setSpeed: (nextSpeed: number | ((current: number) => number)) => {
      if (liveSimulationActive() || liveActive()) return;
      const normalized = normalizeReplaySpeed(
        typeof nextSpeed === "function" ? nextSpeed(speed()) : nextSpeed
      );
      setSpeed(normalized);
      setStreamStartTime(time());
    },
    seek: (nextTime: number) => {
      if (liveSimulationActive() || liveActive()) return;
      const clamped = clampReplayTime(
        nextTime,
        activeMetadata()?.max_t ?? Number.POSITIVE_INFINITY
      );
      setTime(clamped);
      setStreamStartTime(clamped);
      void loadSnapshot(clamped);
    },
    stopLive: async () => {
      openF1LiveCheckRequestId += 1;
      openF1LiveStartRequestId += 1;
      setLiveAvailabilityChecking(false);
      const key = sessionKey();
      const returnKey = sessionKeyAfterLiveStops(key, returnSessionKeyAfterLive);
      openF1LiveStream?.close();
      openF1LiveStream = undefined;
      clearOpenF1LiveReconnect();
      if (key) await api.liveStop(key).catch(() => undefined);
      setLiveActive(false);
      setLiveAvailability("inactive");
      setLiveConnection("idle");
      clearLiveRuntimeResources();
      setSessionKey(returnKey);
      returnSessionKeyAfterLive = undefined;
      setLiveAvailabilityMessage("Live session stopped.");
    },
    checkLive: () => void checkOpenF1Live(),
    clearActiveSession: (nextIntentKey?: number) => {
      if (nextIntentKey === sessionKey()) return;
      stopAbandonedLiveRuntimes();
      openF1LiveCheckRequestId += 1;
      openF1LiveStartRequestId += 1;
      setLiveAvailabilityChecking(false);
      setPlaying(false);
      setLiveActive(false);
      setLiveConnection("idle");
      setLiveSimulationActive(false);
      setLiveSimulationConnection("idle");
      clearLiveRuntimeResources();
      setTime(0);
      setStreamStartTime(0);
      setSnapshotError(undefined);
      initializedSessionKey = undefined;
      returnSessionKeyAfterLive = undefined;
      snapshotRequestId += 1;
      replayStream?.close();
      replayStream = undefined;
      openF1LiveStream?.close();
      openF1LiveStream = undefined;
      clearOpenF1LiveReconnect();
      liveStream?.close();
      liveStream = undefined;
      setSessionKey(undefined);
    },
    openSession: (key: number) => {
      stopAbandonedLiveRuntimes();
      openF1LiveCheckRequestId += 1;
      openF1LiveStartRequestId += 1;
      setLiveAvailabilityChecking(false);
      setPlaying(false);
      setLiveActive(false);
      setLiveConnection("idle");
      setLiveSimulationActive(false);
      setLiveSimulationConnection("idle");
      clearLiveRuntimeResources();
      setTime(0);
      setStreamStartTime(0);
      setSnapshotError(undefined);
      initializedSessionKey = undefined;
      returnSessionKeyAfterLive = undefined;
      openF1LiveStream?.close();
      openF1LiveStream = undefined;
      clearOpenF1LiveReconnect();
      if (shouldReloadSession(sessionKey(), key)) {
        void refetchMetadata();
        void refetchTrackGeometry();
        void refetchEvents();
        return;
      }
      setSessionKey(key);
    },
    toggleLiveSimulation: async () => {
      openF1LiveCheckRequestId += 1;
      openF1LiveStartRequestId += 1;
      setLiveAvailabilityChecking(false);
      const key = sessionKey();
      if (!key) return;
      if (liveSimulationActive()) {
        liveStream?.close();
        liveStream = undefined;
        await api.liveSimulationStop(key).catch(() => undefined);
        setLiveSimulationActive(false);
        setLiveSimulationConnection("idle");
        clearLiveRuntimeResources();
        return;
      }

      setPlaying(false);
      stopAbandonedOpenF1LiveSession();
      setLiveActive(false);
      setLiveConnection("idle");
      setLiveStatus(undefined);
      openF1LiveStream?.close();
      openF1LiveStream = undefined;
      clearOpenF1LiveReconnect();
      setLiveEvents([]);
      setSnapshotError(undefined);
      setSnapshotLoading(true);
      setLiveSimulationConnection("connecting");
      try {
        await api.liveSimulationStart(key);
        const snapshot = await api.liveSimulationSnapshot(key);
        if (snapshot.cursor.session_key === sessionKey()) {
          setCurrentSnapshot(snapshot);
          setTime(snapshot.cursor.t);
        }
        setLiveSimulationActive(true);
      } catch (error) {
        setSnapshotError(error);
        setLiveSimulationConnection("disconnected");
        setLiveSimulationActive(false);
      } finally {
        setSnapshotLoading(false);
      }
    }
  };
}
