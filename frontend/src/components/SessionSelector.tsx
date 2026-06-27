import { batch, createEffect, createResource, createSignal, Show, untrack } from "solid-js";
import type { IngestResponse, Session } from "../../../shared/types/api";
import { api } from "../lib/api";
import {
  canOpenSessionFromCache,
  canOpenSessionAfterIngest,
  ingestOutcome,
  ingestOutcomeClass,
  sessionActionLabel,
  sessionIngestErrorMessage,
  sessionStatusBadgeText,
  sessionStatusClass,
  shouldClearTransientSessionAction,
  type SessionActionState
} from "../lib/sessionReadiness";
import {
  nextMeetingSelection,
  nextSeasonSelection,
  nextSessionSelection,
  readinessForSession,
  shouldSyncActiveSessionSelection,
  meetingOptions,
  selectedSessionLabel,
  seasonOptions,
  sessionOptions
} from "../lib/sessionSelection";
import { SelectField } from "./SelectField";

interface SessionSelectorProps {
  activeSession?: Session;
  activeSessionKey?: number;
  preferredSeason?: number;
  preferredMeeting?: number;
  preferredSession?: number;
  onOpenSession: (sessionKey: number) => void;
  onSelectionChange?: (selection: { sessionKey?: number; label?: string }) => void;
}

export function SessionSelector(props: SessionSelectorProps) {
  const [selectedSeason, setSelectedSeason] = createSignal<number>();
  const [selectedMeeting, setSelectedMeeting] = createSignal<number>();
  const [selectedSession, setSelectedSession] = createSignal<number>();
  const [ingestState, setIngestState] = createSignal<SessionActionState>("idle");
  const [ingestError, setIngestError] = createSignal<string>();
  const [lastIngest, setLastIngest] = createSignal<IngestResponse>();

  const [seasons] = createResource(api.seasons);
  const [meetings] = createResource(selectedSeason, api.meetings);
  const [sessions, { refetch: refetchSessions }] = createResource(selectedMeeting, api.sessions);
  let lastSelectedSession: number | undefined;
  let lastActiveSessionKey: number | undefined;

  createEffect(() => {
    const active = props.activeSession;
    if (!active) return;

    const selected = {
      season: selectedSeason(),
      meeting: selectedMeeting(),
      session: selectedSession()
    };
    if (shouldSyncActiveSessionSelection(active, selected, lastActiveSessionKey)) {
      batch(() => {
        setSelectedSeason(active.year);
        setSelectedMeeting(active.meeting_key);
        setSelectedSession(active.session_key);
      });
    }
    lastActiveSessionKey = active.session_key;
  });

  createEffect(() => {
    const next = nextSeasonSelection(seasons(), selectedSeason(), props.preferredSeason);
    if (next !== selectedSeason()) setSelectedSeason(next);
  });

  createEffect(() => {
    const next = nextMeetingSelection(meetings(), selectedMeeting(), props.preferredMeeting);
    if (next !== selectedMeeting()) setSelectedMeeting(next);
  });

  createEffect(() => {
    const next = nextSessionSelection(sessions(), selectedSession(), props.preferredSession);
    if (next !== selectedSession()) setSelectedSession(next);
  });

  createEffect(() => {
    const nextSession = selectedSession();
    untrack(() => {
      if (
        shouldClearTransientSessionAction({
          previousSession: lastSelectedSession,
          selectedSession: nextSession,
          ingestState: ingestState()
        })
      ) {
        setIngestState("idle");
        setIngestError(undefined);
        setLastIngest(undefined);
      }
      lastSelectedSession = nextSession;
    });
  });

  const selectedReadiness = () =>
    readinessForSession(sessions(), selectedSession());

  createEffect(() => {
    props.onSelectionChange?.({
      sessionKey: selectedSession(),
      label: selectedSessionLabel(selectedReadiness())
    });
  });

  const openSelected = async () => {
    const key = selectedSession();
    if (key == null) return;
    const readiness = selectedReadiness();
    if (canOpenSessionFromCache(readiness)) {
      props.onOpenSession(key);
      setIngestState("idle");
      setIngestError(undefined);
      setLastIngest(undefined);
      return;
    }

    setIngestState("ingesting");
    setIngestError(undefined);
    try {
      const response = await api.ingest(key);
      setLastIngest(response);
      await refetchSessions();
      if (canOpenSessionAfterIngest(response)) {
        props.onOpenSession(key);
        setIngestState("idle");
      } else {
        setIngestError(response.error ?? "Ingest did not produce replay frames.");
        setIngestState("failed");
      }
    } catch (error) {
      setIngestError(error instanceof Error ? error.message : "Ingest failed.");
      void refetchSessions();
      setIngestState("failed");
    }
  };

  const chooseSeason = (season: number) => {
    batch(() => {
      setSelectedSeason(season);
      setSelectedMeeting(undefined);
      setSelectedSession(undefined);
    });
  };

  const chooseMeeting = (meeting: number) => {
    batch(() => {
      setSelectedMeeting(meeting);
      setSelectedSession(undefined);
    });
  };

  const actionLabel = () =>
    sessionActionLabel({
      ingestState: ingestState(),
      selectedSession: selectedSession(),
      activeSessionKey: props.activeSessionKey,
      readiness: selectedReadiness()
    });
  const latestOutcome = () => ingestOutcome(lastIngest());

  return (
    <div
      class="flex items-center gap-2 overflow-x-auto border-b border-line bg-[#101419] px-3 py-2 font-mono text-[0.72rem]"
      data-testid="session-selector"
    >
      <SelectField
        label="Season"
        testId="season-select"
        value={selectedSeason()}
        options={seasonOptions(seasons())}
        disabled={seasons.loading}
        onChange={chooseSeason}
      />

      <SelectField
        label="Meeting"
        testId="meeting-select"
        class="max-w-56 border border-line bg-panel px-2 py-1 text-slate-100 2xl:max-w-64"
        labelClass="ml-2 text-slate-500"
        value={selectedMeeting()}
        options={meetingOptions(meetings())}
        disabled={meetings.loading}
        onChange={chooseMeeting}
      />

      <SelectField
        label="Session"
        testId="session-select"
        labelClass="ml-2 text-slate-500"
        value={selectedSession()}
        options={sessionOptions(sessions())}
        disabled={sessions.loading}
        onChange={setSelectedSession}
      />

      <Show when={selectedReadiness()}>
        {(entry) => (
          <span class={`border px-2 py-1 ${sessionStatusClass(entry().ingest_status)}`}>
            {sessionStatusBadgeText(entry())}
          </span>
        )}
      </Show>

      <button
        class="ml-2 border border-mint bg-mint/10 px-3 py-1 font-semibold text-mint disabled:border-line disabled:text-slate-500"
        data-testid="session-open"
        disabled={selectedSession() == null || ingestState() === "ingesting"}
        onClick={openSelected}
      >
        {actionLabel()}
      </button>

      <Show when={ingestState() === "failed"}>
        <span class="text-danger">
          {sessionIngestErrorMessage({
            ingestError: ingestError(),
            readiness: selectedReadiness()
          })}
        </span>
      </Show>
      <Show when={!sessions.loading && selectedMeeting() != null && (sessions()?.length ?? 0) === 0}>
        <span class="text-amber">No race session available for this meeting.</span>
      </Show>
      <Show when={latestOutcome()}>
        {(outcome) => (
          <span
            class={`max-w-[28rem] truncate ${ingestOutcomeClass(outcome().tone)}`}
            title={outcome().title}
          >
            {outcome().label}
          </span>
        )}
      </Show>
      <Show when={meetings.error || sessions.error}>
        <span class="text-danger">OpenF1 discovery failed.</span>
      </Show>
    </div>
  );
}
