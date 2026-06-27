import { describe, expect, test } from "bun:test";
import type { Meeting, SessionReadiness } from "../../../shared/types/api";
import {
  meetingOptions,
  nextMeetingSelection,
  nextSeasonSelection,
  nextSelection,
  nextSessionSelection,
  readinessForSession,
  activeSessionSelectionMatches,
  selectedSessionLabel,
  shouldSyncActiveSessionSelection,
  seasonOptions,
  sessionDisplayName,
  sessionOptions
} from "./sessionSelection";

describe("nextSelection", () => {
  test("keeps a current selection that still exists", () => {
    expect(nextSelection([{ id: 1 }, { id: 2 }], 2, (item) => item.id)).toBe(2);
  });

  test("falls back to the first option when selection is missing or stale", () => {
    expect(nextSelection([{ id: 1 }, { id: 2 }], undefined, (item) => item.id)).toBe(1);
    expect(nextSelection([{ id: 1 }, { id: 2 }], 9, (item) => item.id)).toBe(1);
  });

  test("uses a preferred option before first-option fallback", () => {
    expect(nextSelection([{ id: 1 }, { id: 2 }], undefined, (item) => item.id, 2)).toBe(2);
    expect(nextSelection([{ id: 1 }, { id: 2 }], 9, (item) => item.id, 2)).toBe(2);
  });

  test("keeps a valid current selection ahead of the preferred option", () => {
    expect(nextSelection([{ id: 1 }, { id: 2 }], 1, (item) => item.id, 2)).toBe(1);
  });

  test("preserves selection while options are unloaded and clears loaded-empty sets", () => {
    expect(nextSelection([], 2, (item: { id: number }) => item.id)).toBeUndefined();
    expect(nextSelection(undefined, 2, (item: { id: number }) => item.id)).toBe(2);
  });
});

describe("specific selection helpers", () => {
  test("select seasons, meetings, and sessions by their stable keys", () => {
    expect(nextSeasonSelection([{ year: 2025 }, { year: 2024 }], 2024)).toBe(2024);
    expect(nextMeetingSelection([meeting(1229), meeting(1230)], 999)).toBe(1229);
    expect(nextSessionSelection([readiness(9472), readiness(9839)], undefined)).toBe(9472);
  });

  test("prefer the curated MVP keys when no current selection is active", () => {
    expect(nextSeasonSelection([{ year: 2026 }, { year: 2024 }], undefined, 2024)).toBe(2024);
    expect(nextMeetingSelection([meeting(1), meeting(1229)], undefined, 1229)).toBe(1229);
    expect(nextSessionSelection([readiness(9839), readiness(9472)], undefined, 9472)).toBe(9472);
  });
});

describe("readinessForSession", () => {
  test("finds the selected readiness entry", () => {
    expect(readinessForSession([readiness(9472), readiness(9839)], 9839)?.session.session_key).toBe(9839);
    expect(readinessForSession([readiness(9472)], undefined)).toBeUndefined();
  });
});

describe("selectedSessionLabel", () => {
  test("formats selected session context for empty replay messages", () => {
    expect(selectedSessionLabel(readiness(9472))).toBe("2024 RACE #9472");
    expect(selectedSessionLabel(readiness(9473, { session_type: "sprint", name: "Sprint" }))).toBe(
      "2024 SPRINT #9473"
    );
    expect(selectedSessionLabel(undefined)).toBeUndefined();
  });
});

describe("activeSessionSelectionMatches", () => {
  test("requires season, meeting, and session to match the active replay", () => {
    const active = readiness(9472).session;

    expect(
      activeSessionSelectionMatches(active, {
        season: 2024,
        meeting: 1229,
        session: 9472
      })
    ).toBe(true);

    expect(
      activeSessionSelectionMatches(active, {
        season: 2024,
        meeting: 1228,
        session: 9472
      })
    ).toBe(false);
  });
});

describe("shouldSyncActiveSessionSelection", () => {
  test("syncs when the active replay session changes", () => {
    const active = readiness(9472).session;

    expect(
      shouldSyncActiveSessionSelection(
        active,
        { season: 2024, meeting: 1228, session: undefined },
        undefined
      )
    ).toBe(true);
  });

  test("repairs stale parent selections for the active session key", () => {
    const active = readiness(9472).session;

    expect(
      shouldSyncActiveSessionSelection(
        active,
        { season: 2024, meeting: 1228, session: 9472 },
        9472
      )
    ).toBe(true);
  });

  test("does not pin the selector when the user browses away from the active session", () => {
    const active = readiness(9472).session;

    expect(
      shouldSyncActiveSessionSelection(
        active,
        { season: 2024, meeting: 1230, session: undefined },
        9472
      )
    ).toBe(false);
  });
});

describe("select option builders", () => {
  test("build compact season and meeting options", () => {
    expect(seasonOptions([{ year: 2024 }, { year: 2023 }])).toEqual([
      { value: 2024, label: "2024" },
      { value: 2023, label: "2023" }
    ]);
    expect(meetingOptions([meeting(1229)])).toEqual([
      { value: 1229, label: "Meeting 1229" }
    ]);
  });

  test("include readiness state in session option labels", () => {
    expect(sessionOptions([readiness(9472, { replay_ready: true })])).toEqual([
      { value: 9472, label: "RACE · ready" }
    ]);
    expect(sessionOptions([readiness(9839, { is_demo: true })])).toEqual([
      { value: 9839, label: "RACE · demo" }
    ]);
    expect(sessionOptions([readiness(9473, { session_type: "sprint", name: "Sprint" })])).toEqual([
      { value: 9473, label: "SPRINT · not ingested" }
    ]);
  });

  test("keeps custom session names after the type label", () => {
    expect(sessionDisplayName(readiness(9473, { session_type: "sprint", name: "Sprint Race" }).session)).toBe(
      "SPRINT Sprint Race"
    );
  });
});

function meeting(meetingKey: number): Meeting {
  return {
    meeting_key: meetingKey,
    year: 2024,
    name: `Meeting ${meetingKey}`,
    country: "",
    location: ""
  };
}

function readiness(
  sessionKey: number,
  overrides: Partial<
    Pick<SessionReadiness, "ingest_status" | "replay_ready" | "is_demo"> & {
      session_type: SessionReadiness["session"]["session_type"];
      name: string;
    }
  > = {}
): SessionReadiness {
  return {
    session: {
      session_key: sessionKey,
      meeting_key: 1229,
      year: 2024,
      name: overrides.name ?? "Race",
      session_type: overrides.session_type ?? "race",
      start_time: "",
      end_time: "",
      total_laps: 57
    },
    ingest_status: overrides.ingest_status ?? "not_ingested",
    replay_ready: overrides.replay_ready ?? false,
    is_demo: overrides.is_demo ?? false,
    last_error: null
  };
}
