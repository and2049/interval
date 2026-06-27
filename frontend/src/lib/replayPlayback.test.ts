import { describe, expect, test } from "bun:test";
import {
  activeReplayMetadata,
  activeReplaySnapshot,
  activeResourceError,
  activeResourceLoading,
  activeTrackGeometry,
  advanceReplayTime,
  clampReplayTime,
  nextReplayTick,
  normalizeReplaySpeed,
  parseReplaySpeedInput,
  parseReplayTimeInput,
  quantizeReplayFrameTime,
  replayResourceSessionKey,
  replayLoadMessage,
  replaySessionTitle,
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
  });
});

describe("replay resource gating", () => {
  test("uses metadata only when it belongs to the active session", () => {
    expect(replayResourceSessionKey(9472, metadata(9472))).toBe(9472);
    expect(replayResourceSessionKey(9839, metadata(9472))).toBeUndefined();
    expect(replayResourceSessionKey(9472, undefined)).toBeUndefined();
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
    expect(activeReplaySnapshot(9472, snapshot(9472))?.cursor.session_key).toBe(9472);
    expect(activeReplaySnapshot(9839, snapshot(9472))).toBeUndefined();
    expect(activeTrackGeometry(9472, geometry(9472))?.session_key).toBe(9472);
    expect(activeTrackGeometry(9839, geometry(9472))).toBeUndefined();
  });

  test("filters stale resource errors by active session", () => {
    const error = new Error("geometry unavailable");

    expect(activeResourceError(9472, 9472, error)).toBe(error);
    expect(activeResourceError(9839, 9472, error)).toBeUndefined();
    expect(activeResourceError(9472, undefined, error)).toBeUndefined();
    expect(activeResourceError(9472, 9472, undefined)).toBeUndefined();
  });

  test("filters stale resource loading states by active session", () => {
    expect(activeResourceLoading(9472, 9472, true)).toBe(true);
    expect(activeResourceLoading(9472, 9472, false)).toBe(false);
    expect(activeResourceLoading(9839, 9472, true)).toBe(false);
    expect(activeResourceLoading(9472, undefined, true)).toBe(false);
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
        sessionKey: 9472,
        preferredHistoricalSessionKey: 9472
      })
    ).toBe("Bahrain replay is not cached yet. Choose INGEST + OPEN to fetch OpenF1 data.");
    expect(
      replayLoadMessage({
        metadataError: "404 Not Found",
        sessionKey: 42,
        preferredHistoricalSessionKey: 9472
      })
    ).toBe("Replay is not cached yet. Choose INGEST + OPEN for this session.");
  });

  test("distinguishes metadata and frame loading states", () => {
    expect(replayLoadMessage({})).toBe("Connecting to replay cache...");
    expect(replayLoadMessage({ metadata: metadata(9472) })).toBe("Loading replay frame...");
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
