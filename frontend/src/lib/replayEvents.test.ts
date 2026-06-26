import { describe, expect, test } from "bun:test";
import type { ReplayEvent } from "../../../shared/types/api";
import {
  eventFeedEmptyLabel,
  eventFeedState,
  eventKindLabel,
  eventSeverityClass,
  recentReplayEvents
} from "./replayEvents";

describe("recentReplayEvents", () => {
  test("returns latest events at or before the replay cursor", () => {
    expect(recentReplayEvents([event("a", 10), event("b", 20), event("c", 30)], 25)).toEqual([
      event("b", 20),
      event("a", 10)
    ]);
  });

  test("limits output and handles invalid cursors", () => {
    expect(recentReplayEvents([event("a", 0), event("b", 1), event("c", 2)], 3, 2)).toEqual([
      event("c", 2),
      event("b", 1)
    ]);
    expect(recentReplayEvents([event("a", 1)], Number.NaN)).toEqual([]);
  });
});

describe("event display helpers", () => {
  test("formats event kind labels and severity classes", () => {
    expect(eventKindLabel("leader_change")).toBe("LEADER CHANGE");
    expect(eventSeverityClass("critical")).toContain("text-danger");
    expect(eventSeverityClass("warning")).toContain("text-amber");
    expect(eventSeverityClass("notice")).toContain("text-mint");
    expect(eventSeverityClass("info")).toContain("text-slate");
  });

  test("labels loading, error, empty, and ready feed states", () => {
    expect(eventFeedState({ rows: [event("a", 1)], loading: true })).toBe("ready");
    expect(eventFeedState({ rows: [], loading: true })).toBe("loading");
    expect(eventFeedState({ rows: [], loading: false, error: new Error("boom") })).toBe("error");
    expect(eventFeedState({ rows: [], loading: false })).toBe("empty");

    expect(eventFeedEmptyLabel("loading")).toBe("Loading replay events...");
    expect(eventFeedEmptyLabel("error")).toBe("Replay event feed unavailable");
    expect(eventFeedEmptyLabel("empty")).toBe("No replay events yet");
    expect(eventFeedEmptyLabel("ready")).toBe("");
  });
});

function event(id: string, t: number): ReplayEvent {
  return {
    id,
    t,
    kind: "race_control",
    severity: "info",
    driver_number: null,
    message: id,
    source: "open_f1",
    payload: {}
  };
}
