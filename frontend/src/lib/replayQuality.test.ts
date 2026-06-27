import { describe, expect, test } from "bun:test";
import type { ReplayMetadata } from "../../../shared/types/api";
import { badgeClass, channelBadges, mapModeClass, mapModeLabel, qualityBadge } from "./replayQuality";

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

function metadata(options: {
  source: ReplayMetadata["track_geometry"]["source"];
  trackReady: boolean;
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
    frame_step_seconds: 1,
    total_frames: 0,
    drivers: [],
    min_t: 0,
    max_t: 0,
    race_start_t: 0,
    generated_at: "",
    data_sources: [],
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
