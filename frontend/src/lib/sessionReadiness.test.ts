import { describe, expect, test } from "bun:test";
import type { SessionReadiness } from "../../../shared/types/api";
import {
  canOpenSessionFromCache,
  canOpenSessionAfterIngest,
  ingestOutcome,
  ingestOutcomeClass,
  sessionIngestErrorMessage,
  sessionActionLabel,
  sessionStatusBadgeText,
  sessionStatusClass,
  sessionStatusLabel,
  shouldClearTransientSessionAction
} from "./sessionReadiness";

describe("sessionStatusLabel", () => {
  test("labels demo and ready sessions before raw ingest status", () => {
    expect(sessionStatusLabel(readiness({ is_demo: true, replay_ready: true }))).toBe("demo");
    expect(sessionStatusLabel(readiness({ replay_ready: true }))).toBe("ready");
  });

  test("humanizes non-ready ingest status", () => {
    expect(sessionStatusLabel(readiness({ ingest_status: "not_ingested" }))).toBe("not ingested");
  });
});

describe("sessionStatusClass", () => {
  test("uses semantic tones for ingest states", () => {
    expect(sessionStatusClass("ready")).toContain("text-mint");
    expect(sessionStatusClass("failed")).toContain("text-danger");
    expect(sessionStatusClass("fetching")).toContain("text-amber");
    expect(sessionStatusClass("not_ingested")).toContain("text-slate");
  });
});

describe("sessionStatusBadgeText", () => {
  test("formats demo and ingest statuses for compact badges", () => {
    expect(sessionStatusBadgeText(readiness({ is_demo: true }))).toBe("DEMO");
    expect(sessionStatusBadgeText(readiness({ ingest_status: "not_ingested" }))).toBe("NOT INGESTED");
  });
});

describe("sessionActionLabel", () => {
  test("labels missing and in-progress selections", () => {
    expect(
      sessionActionLabel({
        ingestState: "idle",
        activeSessionKey: 9472
      })
    ).toBe("SELECT SESSION");

    expect(
      sessionActionLabel({
        ingestState: "ingesting",
        selectedSession: 9472,
        activeSessionKey: 9472
      })
    ).toBe("INGESTING");
  });

  test("distinguishes cached open, cached reload, and ingest", () => {
    expect(
      sessionActionLabel({
        ingestState: "idle",
        selectedSession: 9472,
        activeSessionKey: 9839,
        readiness: readiness({ replay_ready: true })
      })
    ).toBe("OPEN CACHE");

    expect(
      sessionActionLabel({
        ingestState: "idle",
        selectedSession: 9472,
        activeSessionKey: 9472,
        readiness: readiness({ replay_ready: true })
      })
    ).toBe("RELOAD CACHE");

    expect(
      sessionActionLabel({
        ingestState: "idle",
        selectedSession: 9472,
        activeSessionKey: 9839,
        readiness: readiness({ replay_ready: false })
      })
    ).toBe("INGEST + OPEN");
  });
});

describe("canOpenSessionFromCache", () => {
  test("opens ready or demo sessions without ingesting", () => {
    expect(canOpenSessionFromCache(readiness({ replay_ready: true }))).toBe(true);
    expect(canOpenSessionFromCache(readiness({ is_demo: true }))).toBe(true);
    expect(canOpenSessionFromCache(readiness({ replay_ready: false, is_demo: false }))).toBe(false);
    expect(canOpenSessionFromCache(undefined)).toBe(false);
  });
});

describe("canOpenSessionAfterIngest", () => {
  test("opens only ready ingest responses with generated replay frames", () => {
    expect(
      canOpenSessionAfterIngest(ingestResponse({ status: "ready", generated_snapshots: 1200 }))
    ).toBe(true);
    expect(
      canOpenSessionAfterIngest(ingestResponse({ status: "ready", generated_snapshots: 0 }))
    ).toBe(false);
    expect(
      canOpenSessionAfterIngest(ingestResponse({ status: "failed", generated_snapshots: 0 }))
    ).toBe(false);
  });
});

describe("shouldClearTransientSessionAction", () => {
  test("clears stale ingest feedback when the selected session changes", () => {
    expect(
      shouldClearTransientSessionAction({
        previousSession: 9472,
        selectedSession: 9839,
        ingestState: "failed"
      })
    ).toBe(true);
  });

  test("keeps initial selection and in-progress ingest state stable", () => {
    expect(
      shouldClearTransientSessionAction({
        previousSession: undefined,
        selectedSession: 9472,
        ingestState: "failed"
      })
    ).toBe(false);
    expect(
      shouldClearTransientSessionAction({
        previousSession: 9472,
        selectedSession: 9839,
        ingestState: "ingesting"
      })
    ).toBe(false);
  });
});

describe("session ingest feedback helpers", () => {
  test("chooses explicit errors before readiness errors and fallback text", () => {
    expect(sessionIngestErrorMessage({ ingestError: "fetch failed" })).toBe("fetch failed");
    expect(
      sessionIngestErrorMessage({
        readiness: readiness({ last_error: "cached failure" })
      })
    ).toBe("cached failure");
    expect(sessionIngestErrorMessage({})).toBe("Ingest failed.");
  });

  test("summarizes successful ingest output", () => {
    expect(
      ingestOutcome({
        session_key: 9472,
        status: "ready",
        cached_endpoints: 11,
        endpoint_coverage: [],
        generated_snapshots: 1200,
        track_geometry: null,
        available_channels: null,
        warnings: [],
        error: null
      })
    ).toEqual({
      label: "Cached 1,200 frames",
      tone: "ready"
    });
  });

  test("summarizes degraded ingest output with warning details", () => {
    expect(
      ingestOutcome({
        session_key: 9472,
        status: "ready",
        cached_endpoints: 11,
        endpoint_coverage: [],
        generated_snapshots: 1200,
        track_geometry: null,
        available_channels: null,
        warnings: ["location missing", "weather missing"],
        error: null
      })
    ).toEqual({
      label: "Cached 1,200 frames · 2 warnings",
      tone: "degraded",
      title: "location missing | weather missing"
    });
  });

  test("summarizes failed ingest output", () => {
    expect(
      ingestOutcome({
        session_key: 9472,
        status: "failed",
        cached_endpoints: 0,
        endpoint_coverage: [],
        generated_snapshots: 0,
        track_geometry: null,
        available_channels: null,
        warnings: [],
        error: "OpenF1 unavailable"
      })
    ).toEqual({
      label: "OpenF1 unavailable",
      tone: "failed"
    });
    expect(ingestOutcome(undefined)).toBeUndefined();
  });

  test("maps ingest outcome tones to compact text classes", () => {
    expect(ingestOutcomeClass("ready")).toContain("text-mint");
    expect(ingestOutcomeClass("degraded")).toContain("text-amber");
    expect(ingestOutcomeClass("failed")).toContain("text-danger");
  });
});

function readiness(overrides: Partial<SessionReadiness>): SessionReadiness {
  return {
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
    ingest_status: "not_ingested",
    replay_ready: false,
    is_demo: false,
    last_error: null,
    ...overrides
  };
}

function ingestResponse(overrides: {
  status?: "ready" | "failed";
  generated_snapshots?: number;
}) {
  return {
    session_key: 9472,
    status: overrides.status ?? "ready",
    cached_endpoints: 11,
    endpoint_coverage: [],
    generated_snapshots: overrides.generated_snapshots ?? 1200,
    track_geometry: null,
    available_channels: null,
    warnings: [],
    error: overrides.status === "failed" ? "OpenF1 unavailable" : null
  } as const;
}
