import { describe, expect, test } from "bun:test";
import {
  activeReplayMetadata,
  activeReplaySnapshot,
  activeResourceError,
  activeResourceLoading,
  activeTrackGeometry,
  advanceReplayTime,
  clampReplayTime,
  isWaitingForOpenF1LiveDataError,
  liveAvailabilityAfterStartError,
  liveAvailabilityPollDelayMs,
  liveCheckErrorMessage,
  liveCurrentMessage,
  liveSimulationSessionKeyToStop,
  liveStartErrorMessage,
  nextReplayTick,
  openF1LiveSessionKeyToStop,
  normalizeReplaySpeed,
  parseReplaySpeedInput,
  parseReplayTimeInput,
  quantizeReplayFrameTime,
  replayResourceSessionKey,
  replayLoadMessage,
  replaySessionTitle,
  serverSentErrorMessage,
  sessionKeyAfterLiveStops,
  shouldApplyLiveResourceResult,
  shouldApplyLiveStartResult,
  shouldApplySnapshotResult,
  shouldClearMissingHistoricalReplay,
  shouldHideHistoricalResourceError,
  shouldPollLiveAvailability,
  snapshotRequest,
  shouldReloadSession
} from "./replayPlayback";

describe("clampReplayTime", () => {
  test("keeps replay time inside the available range", () => {
    expect(clampReplayTime(-5, 100)).toBe(0);
    expect(clampReplayTime(105, 100)).toBe(100);
    expect(clampReplayTime(25, 100)).toBe(25);
  });

  test("handles invalid numeric input without leaking NaN", () => {
    expect(clampReplayTime(Number.NaN, 100)).toBe(0);
    expect(clampReplayTime(12, Number.NaN)).toBe(12);
  });
});

describe("advanceReplayTime", () => {
  test("advances by elapsed time and normalized playback speed", () => {
    expect(advanceReplayTime(10, 2, 4, 100)).toBe(18);
    expect(advanceReplayTime(98, 2, 4, 100)).toBe(100);
  });

  test("ignores invalid elapsed values", () => {
    expect(advanceReplayTime(10, -1, 2, 100)).toBe(10);
    expect(advanceReplayTime(10, Number.NaN, 2, 100)).toBe(10);
  });
});

describe("normalizeReplaySpeed", () => {
  test("accepts supported speeds and falls back for unsupported values", () => {
    expect(normalizeReplaySpeed(0.5)).toBe(0.5);
    expect(normalizeReplaySpeed(4)).toBe(4);
    expect(normalizeReplaySpeed(3)).toBe(1);
    expect(normalizeReplaySpeed(Number.NaN)).toBe(1);
  });
});

describe("replay control input parsing", () => {
  test("parses numeric seek input and ignores empty or invalid values", () => {
    expect(parseReplayTimeInput("42.5")).toBe(42.5);
    expect(parseReplayTimeInput("")).toBeUndefined();
    expect(parseReplayTimeInput("   ")).toBeUndefined();
    expect(parseReplayTimeInput("not-a-time")).toBeUndefined();
  });

  test("normalizes speed input from the control", () => {
    expect(parseReplaySpeedInput("2")).toBe(2);
    expect(parseReplaySpeedInput("3")).toBe(1);
    expect(parseReplaySpeedInput("")).toBe(1);
  });
});

describe("quantizeReplayFrameTime", () => {
  test("uses the replay frame cadence to request persisted frames", () => {
    const replayMetadata = metadata(9472, { frameStepSeconds: 0.5, maxT: 102 });

    expect(quantizeReplayFrameTime(0, replayMetadata)).toBe(0);
    expect(quantizeReplayFrameTime(0.49, replayMetadata)).toBe(0);
    expect(quantizeReplayFrameTime(0.5, replayMetadata)).toBe(0.5);
    expect(quantizeReplayFrameTime(101.9, replayMetadata)).toBe(101.5);
  });

  test("falls back to clamping when metadata has no usable frame cadence", () => {
    const replayMetadata = metadata(9472, { frameStepSeconds: 0, maxT: 100 });

    expect(quantizeReplayFrameTime(12.5, replayMetadata)).toBe(12.5);
    expect(quantizeReplayFrameTime(150, replayMetadata)).toBe(100);
  });
});

describe("nextReplayTick", () => {
  test("keeps idle or unloaded replay state unchanged", () => {
    expect(
      nextReplayTick({
        currentTime: 10,
        elapsedSeconds: 5,
        speed: 2,
        playing: false,
        maxT: 100
      })
    ).toEqual({ time: 10, playing: false });

    expect(
      nextReplayTick({
        currentTime: 10,
        elapsedSeconds: 5,
        speed: 2,
        playing: true
      })
    ).toEqual({ time: 10, playing: true });
  });

  test("advances playing replay state and stops at the end", () => {
    expect(
      nextReplayTick({
        currentTime: 10,
        elapsedSeconds: 5,
        speed: 2,
        playing: true,
        maxT: 100
      })
    ).toEqual({ time: 20, playing: true });

    expect(
      nextReplayTick({
        currentTime: 98,
        elapsedSeconds: 5,
        speed: 1,
        playing: true,
        maxT: 100
      })
    ).toEqual({ time: 100, playing: false });
  });
});

describe("shouldReloadSession", () => {
  test("distinguishes same-session reload from session switch", () => {
    expect(shouldReloadSession(9472, 9472)).toBe(true);
    expect(shouldReloadSession(9839, 9472)).toBe(false);
    expect(shouldReloadSession(undefined, 9472)).toBe(false);
  });
});

describe("replay resource gating", () => {
  test("uses metadata only when it belongs to the active session", () => {
    expect(replayResourceSessionKey(9472, metadata(9472))).toBe(9472);
    expect(replayResourceSessionKey(9839, metadata(9472))).toBeUndefined();
    expect(replayResourceSessionKey(9472, undefined)).toBeUndefined();
    expect(replayResourceSessionKey(undefined, metadata(9472))).toBeUndefined();
  });

  test("builds snapshot requests only from active-session metadata", () => {
    expect(
      snapshotRequest(9472, metadata(9472, { frameStepSeconds: 0.5, maxT: 100 }), 27.4)
    ).toEqual({ key: 9472, t: 27 });
    expect(snapshotRequest(9839, metadata(9472), 25)).toBeUndefined();
  });

  test("filters stale resource values by active session", () => {
    expect(activeReplayMetadata(9472, metadata(9472))?.session.session_key).toBe(9472);
    expect(activeReplayMetadata(9839, metadata(9472))).toBeUndefined();
    expect(activeReplayMetadata(undefined, metadata(9472))).toBeUndefined();
    expect(activeReplaySnapshot(9472, snapshot(9472))?.cursor.session_key).toBe(9472);
    expect(activeReplaySnapshot(9839, snapshot(9472))).toBeUndefined();
    expect(activeReplaySnapshot(undefined, snapshot(9472))).toBeUndefined();
    expect(activeTrackGeometry(9472, geometry(9472))?.session_key).toBe(9472);
    expect(activeTrackGeometry(9839, geometry(9472))).toBeUndefined();
    expect(activeTrackGeometry(undefined, geometry(9472))).toBeUndefined();
  });

  test("filters stale resource errors by active session", () => {
    const error = new Error("geometry unavailable");

    expect(activeResourceError(9472, 9472, error)).toBe(error);
    expect(activeResourceError(9839, 9472, error)).toBeUndefined();
    expect(activeResourceError(9472, undefined, error)).toBeUndefined();
    expect(activeResourceError(undefined, 9472, error)).toBeUndefined();
    expect(activeResourceError(9472, 9472, undefined)).toBeUndefined();
  });

  test("filters stale resource loading states by active session", () => {
    expect(activeResourceLoading(9472, 9472, true)).toBe(true);
    expect(activeResourceLoading(9472, 9472, false)).toBe(false);
    expect(activeResourceLoading(9839, 9472, true)).toBe(false);
    expect(activeResourceLoading(9472, undefined, true)).toBe(false);
    expect(activeResourceLoading(undefined, 9472, true)).toBe(false);
  });
});

describe("replayLoadMessage", () => {
  test("uses specific errors when available", () => {
    expect(replayLoadMessage({ metadataError: new Error("metadata missing") })).toBe("metadata missing");
    expect(replayLoadMessage({ snapshotError: "snapshot missing" })).toBe("snapshot missing");
  });

  test("keeps stale errors hidden while the next replay resource is loading", () => {
    expect(
      replayLoadMessage({
        metadataLoading: true,
        metadataError: new Error("previous metadata error")
      })
    ).toBe("Connecting to replay cache...");

    expect(
      replayLoadMessage({
        metadata: metadata(9472),
        snapshotLoading: true,
        snapshotError: new Error("previous snapshot error")
      })
    ).toBe("Loading replay frame...");
  });

  test("turns missing historical metadata into an ingest prompt", () => {
    expect(
      replayLoadMessage({
        metadataError: new Error("resource not found"),
        sessionKey: 9472
      })
    ).toBe("Replay is not cached yet. Select a supported race or sprint to ingest it.");
    expect(
      replayLoadMessage({
        metadataError: "404 Not Found",
        sessionKey: 42,
        selectedSessionLabel: "2025 Race #42"
      })
    ).toBe("No cached replay for selected race: 2025 Race #42. Ingest starts automatically when supported.");
  });

  test("explains empty selection and selected uncached states", () => {
    expect(replayLoadMessage({ sessionKey: undefined })).toBe(
      "Live races open automatically when available. Select a historical race or sprint to replay."
    );
    expect(
      replayLoadMessage({
        sessionKey: undefined,
        liveStatusMessage: "Checking live race status..."
      })
    ).toBe("Checking live race status...");
    expect(
      replayLoadMessage({
        sessionKey: undefined,
        selectedSessionLabel: "2025 Race #1234",
        liveStatusMessage: "No active live race or sprint"
      })
    ).toBe("No cached replay for selected race: 2025 Race #1234. Ingest starts automatically when supported.");
  });

  test("distinguishes metadata and frame loading states", () => {
    expect(replayLoadMessage({ sessionKey: 9472 })).toBe("Connecting to replay cache...");
    expect(replayLoadMessage({ metadata: metadata(9472) })).toBe("Loading replay frame...");
  });

  test("describes a live connection instead of a historical frame load", () => {
    expect(
      replayLoadMessage({
        metadata: metadata(9472),
        snapshotLoading: true,
        liveConnecting: true,
        liveStatusMessage: "Opening active OpenF1 live session..."
      })
    ).toBe("Opening active OpenF1 live session...");
  });
});

describe("shouldClearMissingHistoricalReplay", () => {
  test("clears stale historical sessions when cached metadata is missing", () => {
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: new Error("resource not found"),
        sessionKey: 9472
      })
    ).toBe(true);
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: "404 Not Found",
        sessionKey: 9472
      })
    ).toBe(true);
  });

  test("does not clear active live sessions when historical metadata is missing", () => {
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: new Error("resource not found"),
        sessionKey: 88001,
        liveActive: true
      })
    ).toBe(false);
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: "404 Not Found",
        sessionKey: 9839,
        liveSimulationActive: true
      })
    ).toBe(false);
  });

  test("ignores loading, non-missing errors, and empty session state", () => {
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: new Error("resource not found"),
        metadataLoading: true,
        sessionKey: 9472
      })
    ).toBe(false);
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: new Error("database unavailable"),
        sessionKey: 9472
      })
    ).toBe(false);
    expect(
      shouldClearMissingHistoricalReplay({
        metadataError: new Error("resource not found")
      })
    ).toBe(false);
  });
});

describe("shouldApplyLiveStartResult", () => {
  test("only applies the latest live start request", () => {
    expect(shouldApplyLiveStartResult(3, 3)).toBe(true);
    expect(shouldApplyLiveStartResult(2, 3)).toBe(false);
  });
});

describe("shouldApplySnapshotResult", () => {
  test("rejects stale or live-mode historical snapshot results", () => {
    expect(shouldApplySnapshotResult(3, 3, 9472, 9472, false, false)).toBe(true);
    expect(shouldApplySnapshotResult(2, 3, 9472, 9472, false, false)).toBe(false);
    expect(shouldApplySnapshotResult(3, 3, 9472, 9839, false, false)).toBe(false);
    expect(shouldApplySnapshotResult(3, 3, 9472, 9472, true, false)).toBe(false);
    expect(shouldApplySnapshotResult(3, 3, 9472, 9472, false, true)).toBe(false);
    expect(shouldApplySnapshotResult(3, 3, 9472, 9472, false, false, true)).toBe(false);
  });
});

describe("shouldApplyLiveResourceResult", () => {
  test("applies live resources only for the active live session", () => {
    expect(shouldApplyLiveResourceResult(88001, 88001, true)).toBe(true);
    expect(shouldApplyLiveResourceResult(88001, 9472, true)).toBe(false);
    expect(shouldApplyLiveResourceResult(88001, 88001, false)).toBe(false);
    expect(shouldApplyLiveResourceResult(undefined, 88001, true)).toBe(false);
  });
});

describe("shouldHideHistoricalResourceError", () => {
  test("hides historical cache errors while real live mode owns resources", () => {
    expect(shouldHideHistoricalResourceError(true)).toBe(true);
    expect(shouldHideHistoricalResourceError(false)).toBe(false);
  });
});

describe("sessionKeyAfterLiveStops", () => {
  test("returns a previous replay session only when it differs from the live session", () => {
    expect(sessionKeyAfterLiveStops(88001, 9472)).toBe(9472);
    expect(sessionKeyAfterLiveStops(88001, 88001)).toBeUndefined();
    expect(sessionKeyAfterLiveStops(88001, undefined)).toBeUndefined();
  });
});

describe("openF1LiveSessionKeyToStop", () => {
  test("returns the active live key when leaving OpenF1 live mode", () => {
    expect(openF1LiveSessionKeyToStop(88001, true)).toBe(88001);
    expect(openF1LiveSessionKeyToStop(88001, false)).toBeUndefined();
    expect(openF1LiveSessionKeyToStop(undefined, true)).toBeUndefined();
  });
});

describe("liveSimulationSessionKeyToStop", () => {
  test("returns the active simulation key when leaving live simulation mode", () => {
    expect(liveSimulationSessionKeyToStop(9472, true)).toBe(9472);
    expect(liveSimulationSessionKeyToStop(9472, false)).toBeUndefined();
    expect(liveSimulationSessionKeyToStop(undefined, true)).toBeUndefined();
  });
});

describe("replaySessionTitle", () => {
  test("uses meeting context when metadata includes it", () => {
    expect(replaySessionTitle(metadata(9472))).toBe("2024 Bahrain Grand Prix · Race");
  });

  test("falls back to session identity when meeting context is unavailable", () => {
    const replayMetadata = metadata(9472);
    expect(replaySessionTitle({ ...replayMetadata, meeting: null })).toBe("2024 Race · #9472");
  });
});

describe("serverSentErrorMessage", () => {
  test("extracts backend SSE error payloads", () => {
    expect(serverSentErrorMessage({ data: "live snapshot refresh failed" } as MessageEvent)).toBe(
      "live snapshot refresh failed"
    );
  });

  test("ignores transport-style error events without payload data", () => {
    expect(serverSentErrorMessage(new Event("error"))).toBeUndefined();
    expect(serverSentErrorMessage({ data: "" } as MessageEvent)).toBeUndefined();
  });
});

describe("liveCurrentMessage", () => {
  test("uses backend-provided live availability messages when present", () => {
    expect(liveCurrentMessage("error", "OpenF1 live configuration error")).toBe(
      "OpenF1 live configuration error"
    );
    expect(liveCurrentMessage("inactive", "No race today")).toBe("No race today");
  });

  test("labels the next live candidate when inactive", () => {
    const next = metadata(88001);
    expect(
      liveCurrentMessage(
        "inactive",
        undefined,
        { ...next.session, start_time: "2024-03-02T15:00:00Z" },
        next.meeting
      )
    ).toBe("Next live: 2024 Bahrain Grand Prix · Race · 2024-03-02 15:00 UTC");
  });

  test("omits the next live start time when the timestamp is invalid", () => {
    const next = metadata(88001);
    expect(
      liveCurrentMessage(
        "inactive",
        undefined,
        { ...next.session, start_time: "not-a-date" },
        next.meeting
      )
    ).toBe("Next live: 2024 Bahrain Grand Prix · Race");
  });

  test("falls back to concise live availability labels", () => {
    expect(liveCurrentMessage("disabled")).toBe("LIVE disabled");
    expect(liveCurrentMessage("inactive")).toBe("No active live race or sprint");
    expect(liveCurrentMessage("error")).toBe("OpenF1 live status unavailable");
    expect(liveCurrentMessage("active")).toBeUndefined();
  });
});

describe("liveCheckErrorMessage", () => {
  test("preserves thrown live-check errors for actionable feedback", () => {
    expect(liveCheckErrorMessage(new Error("OpenF1 live configuration error"))).toBe(
      "OpenF1 live configuration error"
    );
    expect(liveCheckErrorMessage("network unavailable")).toBe("network unavailable");
  });
});

describe("liveStartErrorMessage", () => {
  test("preserves live-start backend errors for actionable feedback", () => {
    expect(liveStartErrorMessage(new Error("OpenF1 live initial snapshot has no driver data"))).toBe(
      "Waiting for OpenF1 live data. OpenF1 live initial snapshot has no driver data"
    );
    expect(liveStartErrorMessage(undefined)).toBe("OpenF1 live session could not be opened.");
  });

  test("identifies temporary OpenF1 live warmup errors", () => {
    expect(isWaitingForOpenF1LiveDataError("OpenF1 live initial snapshot has no timing/location data")).toBe(true);
    expect(isWaitingForOpenF1LiveDataError("OpenF1 live request failed")).toBe(false);
  });

  test("keeps temporary live warmup failures in the retryable availability state", () => {
    expect(liveAvailabilityAfterStartError("OpenF1 live initial snapshot has no driver data")).toBe("inactive");
    expect(liveAvailabilityAfterStartError("OpenF1 live request failed")).toBe("error");
  });
});

describe("shouldPollLiveAvailability", () => {
  test("retries inactive and transient error states only", () => {
    expect(shouldPollLiveAvailability("inactive")).toBe(true);
    expect(shouldPollLiveAvailability("error")).toBe(true);
    expect(shouldPollLiveAvailability("disabled")).toBe(false);
    expect(shouldPollLiveAvailability("active")).toBe(false);
  });
});

describe("liveAvailabilityPollDelayMs", () => {
  test("keeps warmup checks responsive but backs off ordinary inactive and error states", () => {
    expect(
      liveAvailabilityPollDelayMs(
        "inactive",
        "Waiting for OpenF1 live data. OpenF1 live initial snapshot has no driver data"
      )
    ).toBe(10_000);
    expect(liveAvailabilityPollDelayMs("inactive", "No active live race or sprint")).toBe(60_000);
    expect(liveAvailabilityPollDelayMs("error", "OpenF1 live status unavailable")).toBe(60_000);
  });

  test("does not schedule polling for terminal availability states", () => {
    expect(liveAvailabilityPollDelayMs("disabled")).toBe(Number.POSITIVE_INFINITY);
    expect(liveAvailabilityPollDelayMs("active")).toBe(Number.POSITIVE_INFINITY);
  });
});

function metadata(
  sessionKey = 9472,
  options: { frameStepSeconds?: number; maxT?: number } = {}
) {
  return {
    contract_version: "replay.v1",
    session: {
      session_key: sessionKey,
      meeting_key: 1229,
      year: 2024,
      name: "Race",
      session_type: "race",
      start_time: "",
      end_time: "",
      total_laps: 57
    },
    meeting: {
      meeting_key: 1229,
      year: 2024,
      name: "Bahrain Grand Prix",
      country: "Bahrain",
      location: "Sakhir"
    },
    duration_seconds: 0,
    frame_step_seconds: options.frameStepSeconds ?? 1,
    total_frames: 0,
    drivers: [],
    min_t: 0,
    max_t: options.maxT ?? 0,
    race_start_t: 0,
    generated_at: "",
    data_sources: [],
    available_channels: {
      timing: true,
      location: false,
      track_geometry: true,
      weather: false,
      race_control: false,
      stints: false,
      pit_events: false,
      intervals: false
    },
    track_geometry: {
      status: "ready",
      source: "curated_static",
      quality: "ready"
    },
    endpoints: {
      snapshot_endpoint: "",
      stream_endpoint: "",
      events_endpoint: "",
      track_geometry_endpoint: ""
    }
  } as const;
}

function snapshot(sessionKey = 9472) {
  return {
    contract_version: "replay.v1",
    cursor: {
      session_key: sessionKey,
      t: 0,
      frame_index: 0,
      playback_speed: 1,
      is_paused: true
    },
    race_state: {
      lap: 1,
      track_status: "green"
    },
    timing: {
      rows: [],
      quality: "missing"
    },
    track: {
      positions: [],
      map_mode: "schematic",
      quality: "missing"
    },
    weather: {
      sample: null,
      quality: "missing"
    },
    race_control: {
      messages: [],
      quality: "missing"
    },
    derived_metrics: []
  } as const;
}

function geometry(sessionKey = 9472) {
  return {
    contract_version: "replay.v1",
    session_key: sessionKey,
    bounds: {
      min_x: 0,
      max_x: 1,
      min_y: 0,
      max_y: 1
    },
    centerline: [],
    inner_edge: [],
    outer_edge: [],
    source: "schematic",
    quality: "schematic",
    map_mode: "schematic",
    circuit_length: null,
    generated_at: ""
  } as const;
}
