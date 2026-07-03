import { describe, expect, test } from "bun:test";
import type { ReplayMetadata } from "../../../shared/types/api";
import {
  badgeClass,
  channelBadges,
  liveChannelBadges,
  liveDashboardBadges,
  liveStatusLabel,
  mapModeClass,
  mapModeLabel,
  qualityBadge
} from "./replayQuality";

describe("mapModeLabel", () => {
  test("labels map modes for compact UI badges", () => {
    expect(mapModeLabel("gps")).toBe("MAP GPS");
    expect(mapModeLabel("projected")).toBe("MAP PROJECTED");
    expect(mapModeLabel("schematic")).toBe("MAP SCHEMATIC");
  });
});

describe("mapModeClass", () => {
  test("uses semantic tones for map modes", () => {
    expect(mapModeClass("gps")).toContain("text-mint");
    expect(mapModeClass("projected")).toContain("text-amber");
    expect(mapModeClass("schematic")).toContain("text-slate");
  });
});

describe("channelBadges", () => {
  test("marks curated static geometry as projected and degraded", () => {
    const badges = channelBadges(metadata({ source: "curated_static", trackReady: true }));

    expect(badges.find((badge) => badge.label === "MAP PROJECTED")).toMatchObject({
      ready: true,
      tone: "degraded"
    });
  });

  test("marks OpenF1 location geometry as GPS and ready", () => {
    const badges = channelBadges(metadata({ source: "open_f1_location", trackReady: true }));

    expect(badges.find((badge) => badge.label === "MAP GPS")).toMatchObject({
      ready: true,
      tone: "ready"
    });
  });

  test("marks FastF1 telemetry geometry as GPS and ready", () => {
    const badges = channelBadges(metadata({ source: "fast_f1_telemetry", trackReady: true }));

    expect(badges.find((badge) => badge.label === "MAP GPS")).toMatchObject({
      ready: true,
      tone: "ready"
    });
  });

  test("keeps unavailable channels visually muted", () => {
    expect(badgeClass("missing")).toContain("text-slate");
  });

  test("adds source, cadence, and cache state badges", () => {
    const badges = channelBadges(
      metadata({
        source: "fast_f1_telemetry",
        trackReady: true,
        dataSource: "fastf1_historical",
        frameStepSeconds: 0.2
      })
    );

    expect(badges[0]).toMatchObject({ label: "FastF1 · 5 Hz", tone: "ready" });
    expect(badges[1]).toMatchObject({ label: "DEGRADED", tone: "degraded" });
  });

  test("labels live simulation as a ready live-like source", () => {
    const badges = channelBadges(
      metadata({
        source: "fast_f1_telemetry",
        trackReady: true,
        dataSource: "live_simulation",
        frameStepSeconds: 0.2
      })
    );

    expect(badges[0]).toMatchObject({ label: "Live Sim · 5 Hz", tone: "ready" });
  });

  test("labels OpenF1 live as a ready live source", () => {
    const badges = channelBadges(
      metadata({
        source: "open_f1_location",
        trackReady: true,
        dataSource: "openf1_live",
        frameStepSeconds: 0.5
      })
    );

    expect(badges[0]).toMatchObject({ label: "LIVE · OpenF1 · 2 Hz", tone: "ready" });
  });
});

describe("qualityBadge", () => {
  test("maps section quality to compact badge tones", () => {
    expect(qualityBadge("ready")).toEqual({ label: "READY", ready: true, tone: "ready" });
    expect(qualityBadge("projected")).toEqual({
      label: "PROJECTED",
      ready: false,
      tone: "degraded"
    });
    expect(qualityBadge("missing")).toEqual({
      label: "MISSING",
      ready: false,
      tone: "missing"
    });
  });
});

describe("liveChannelBadges", () => {
  test("maps live endpoint health to compact badge tones", () => {
    const badges = liveChannelBadges([
      { endpoint: "location", state: "fresh", age_seconds: 0.2, rows: 20, last_error: null },
      { endpoint: "weather", state: "cached", age_seconds: 5, rows: 1, last_error: null },
      { endpoint: "pit", state: "cached", age_seconds: 5, rows: 1, last_error: "timeout" },
      { endpoint: "drivers", state: "fresh", age_seconds: -1, rows: 20, last_error: null },
      { endpoint: "pit", state: "fresh", age_seconds: 0.1, rows: 0, last_error: null },
      { endpoint: "intervals", state: "stale", age_seconds: 12, rows: 20, last_error: null },
      { endpoint: "race_control", state: "missing", age_seconds: 1, rows: 0, last_error: null }
    ]);

    expect(badges).toEqual([
      { label: "LOCATION", ready: true, tone: "ready", title: "location: fresh · 20 rows · 0s old" },
      { label: "WEATHER", ready: true, tone: "ready", title: "weather: cached · 1 rows · 5s old" },
      { label: "PIT", ready: true, tone: "degraded", title: "pit: cached · 1 rows · 5s old · timeout" },
      { label: "DRIVERS", ready: true, tone: "ready", title: "drivers: fresh · 20 rows · 0s old" },
      { label: "PIT", ready: true, tone: "ready", title: "pit: fresh · 0 rows · 0s old" },
      { label: "INTERVALS", ready: false, tone: "degraded", title: "intervals: stale · 20 rows · 12s old" },
      { label: "RACE_CONTROL", ready: false, tone: "missing", title: "race_control: missing · 0 rows · 1s old" }
    ]);
  });
});

describe("liveDashboardBadges", () => {
  test("keeps the live source and cadence badge before endpoint health", () => {
    const badges = liveDashboardBadges(
      metadata({
        source: "open_f1_location",
        trackReady: true,
        dataSource: "openf1_live",
        frameStepSeconds: 0.5
      }),
      [{ endpoint: "location", state: "fresh", age_seconds: 0.2, rows: 20, last_error: null }]
    );

    expect(badges[0]).toMatchObject({ label: "LIVE · OpenF1 · 2 Hz", tone: "ready" });
    expect(badges[1]).toMatchObject({ label: "LOCATION", tone: "ready" });
  });
});

describe("liveStatusLabel", () => {
  test("includes latest backend update age when available", () => {
    expect(
      liveStatusLabel(
        "connected",
        {
          session_key: 88_001,
          active: true,
          current_t: 10,
          max_t: 100,
          started_at: "2026-06-28T20:00:00.000Z",
          updated_at: "2026-06-28T20:00:03.000Z",
          source: "openf1_live",
          channels: []
        },
        Date.parse("2026-06-28T20:00:04.200Z")
      )
    ).toBe("LIVE connected · UPDATED 1s");
  });

  test("falls back to connection state when update timestamp is missing", () => {
    expect(liveStatusLabel("reconnecting")).toBe("LIVE reconnecting");
  });
});

function metadata(options: {
  source: ReplayMetadata["track_geometry"]["source"];
  trackReady: boolean;
  dataSource?: string;
  frameStepSeconds?: number;
}): ReplayMetadata {
  return {
    contract_version: "replay.v1",
    session: {
      session_key: 9472,
      meeting_key: 1229,
      year: 2024,
      name: "Race",
      session_type: "race",
      start_time: "",
      end_time: "",
      total_laps: 57
    },
    duration_seconds: 0,
    frame_step_seconds: options.frameStepSeconds ?? 1,
    total_frames: 0,
    drivers: [],
    min_t: 0,
    max_t: 0,
    race_start_t: 0,
    generated_at: "",
    data_sources: options.dataSource
      ? [{ name: options.dataSource, mode: "historical" }]
      : [],
    available_channels: {
      timing: true,
      location: options.source === "open_f1_location" || options.source === "fast_f1_telemetry",
      track_geometry: options.trackReady,
      weather: false,
      race_control: false,
      stints: false,
      pit_events: false,
      intervals: false
    },
    track_geometry: {
      status: options.trackReady ? "ready" : "missing",
      source: options.source,
      quality: options.trackReady ? "ready" : "missing"
    },
    endpoints: {
      snapshot_endpoint: "",
      stream_endpoint: "",
      events_endpoint: "",
      track_geometry_endpoint: ""
    }
  };
}
