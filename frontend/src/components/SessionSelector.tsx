import { batch, createEffect, createResource, createSignal, Show, untrack } from "solid-js";
import type { IngestResponse, Session } from "../../../shared/types/api";
import { api } from "../lib/api";
import {
  canOpenSessionFromCache,
  canOpenSessionAfterIngest,
  canStartSessionAction,
  ingestOutcome,
  ingestOutcomeClass,
  isSessionActionDisabled,
  sessionActionLabel,
  sessionActionStatus,
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
  onSessionIntent?: (sessionKey?: number) => void;
  onSelectionChange?: (selection: { sessionKey?: number; label?: string }) => void;
  liveStatusMessage?: string;
  liveChecking?: boolean;
  liveActive?: boolean;
  onCheckLive?: () => void;
}

export function SessionSelector(props: SessionSelectorProps) {
  const [selectedSeason, setSelectedSeason] = createSignal<number>();
  const [selectedMeeting, setSelectedMeeting] = createSignal<number>();
  const [selectedSession, setSelectedSession] = createSignal<number>();
  const [ingestState, setIngestState] = createSignal<SessionActionState>("idle");
  const [ingestError, setIngestError] = createSignal<string>();
  const [lastIngest, setLastIngest] = createSignal<IngestResponse>();
  const [sessionIntent, setSessionIntent] = createSignal(0);
  const [meetingIntent, setMeetingIntent] = createSignal(0);
  const [hasUserBrowsed, setHasUserBrowsed] = createSignal(false);

  const [seasons] = createResource(api.seasons);
  const [meetings] = createResource(selectedSeason, api.meetings);
  const [sessions, { refetch: refetchSessions }] = createResource(selectedMeeting, api.sessions);
  let lastSelectedSession: number | undefined;
  let lastActiveSessionKey: number | undefined;
  let handledSessionIntent = 0;
  let handledMeetingIntent = 0;
  let sessionActionRequestId = 0;

  createEffect(() => {
    const active = props.activeSession;
    if (!active) return;
    if (hasUserBrowsed() && selectedSession() !== active.session_key) return;

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

  createEffect(() => {
    const intent = meetingIntent();
    const key = selectedSession();
    const readiness = selectedReadiness();
    if (
      intent === 0 ||
      intent === handledMeetingIntent ||
      sessions.loading ||
      key == null ||
      !readiness
    ) {
      return;
    }

    handledMeetingIntent = intent;
    setSessionIntent((value) => value + 1);
  });

  createEffect(() => {
    const intent = sessionIntent();
    const key = selectedSession();
    const readiness = selectedReadiness();
    if (intent === 0 || intent === handledSessionIntent || key == null || !readiness) return;

    handledSessionIntent = intent;
    void openSelected("auto");
  });

  const openSelected = async (_source: "auto" | "manual" = "manual") => {
    const key = selectedSession();
    if (key == null) return;
    const requestId = ++sessionActionRequestId;
    const readiness = selectedReadiness();
    props.onSessionIntent?.(key);

    if (!canStartSessionAction(readiness)) {
      setIngestState("failed");
      setIngestError(readiness?.support_reason ?? "Selected session is unavailable.");
      return;
    }

    setIngestState("checking");
    setIngestError(undefined);
    if (canOpenSessionFromCache(readiness)) {
      if (requestId !== sessionActionRequestId || selectedSession() !== key) return;
      setIngestState("opening_cache");
      props.onOpenSession(key);
      if (requestId !== sessionActionRequestId || selectedSession() !== key) return;
      setIngestError(undefined);
      setLastIngest(undefined);
      setIngestState("idle");
      return;
    }

    setIngestState("ingesting");
    setIngestError(undefined);
    try {
      const response = await api.ingest(key);
      if (requestId !== sessionActionRequestId || selectedSession() !== key) return;
      setLastIngest(response);
      await refetchSessions();
      if (requestId !== sessionActionRequestId || selectedSession() !== key) return;
      if (canOpenSessionAfterIngest(response)) {
        setIngestState("opening_replay");
        props.onOpenSession(key);
        if (requestId !== sessionActionRequestId || selectedSession() !== key) return;
        setIngestState("idle");
      } else {
        setIngestError(response.error ?? "Ingest did not produce replay frames.");
        setIngestState("failed");
      }
    } catch (error) {
      if (requestId !== sessionActionRequestId || selectedSession() !== key) return;
      setIngestError(error instanceof Error ? error.message : "Ingest failed.");
      void refetchSessions();
      setIngestState("failed");
    }
  };

  const chooseSeason = (season: number) => {
    setHasUserBrowsed(true);
    sessionActionRequestId += 1;
    props.onSessionIntent?.();
    batch(() => {
      setSelectedSeason(season);
      setSelectedMeeting(undefined);
      setSelectedSession(undefined);
    });
  };

  const chooseMeeting = (meeting: number) => {
    setHasUserBrowsed(true);
    sessionActionRequestId += 1;
    props.onSessionIntent?.();
    batch(() => {
      setSelectedMeeting(meeting);
      setSelectedSession(undefined);
      setMeetingIntent((value) => value + 1);
    });
  };

  const chooseSession = (session: number) => {
    setHasUserBrowsed(true);
    batch(() => {
      setSelectedSession(session);
      setSessionIntent((value) => value + 1);
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
  const selectorLocked = () => props.liveActive === true;

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
        disabled={seasons.loading || selectorLocked()}
        onChange={chooseSeason}
      />

      <SelectField
        label="Meeting"
        testId="meeting-select"
        class="max-w-56 border border-line bg-panel px-2 py-1 text-slate-100 2xl:max-w-64"
        labelClass="ml-2 text-slate-500"
        value={selectedMeeting()}
        options={meetingOptions(meetings())}
        disabled={meetings.loading || selectorLocked()}
        onChange={chooseMeeting}
      />

      <SelectField
        label="Session"
        testId="session-select"
        labelClass="ml-2 text-slate-500"
        value={selectedSession()}
        options={sessionOptions(sessions())}
        disabled={sessions.loading || selectorLocked()}
        onChange={chooseSession}
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
        disabled={
          isSessionActionDisabled({
            selectedSession: selectedSession(),
            readiness: selectedReadiness(),
            ingestState: ingestState(),
            liveActive: props.liveActive
          })
        }
        onClick={() => void openSelected("manual")}
      >
        {actionLabel()}
      </button>

      <button
        class="border border-line bg-panel px-3 py-1 font-semibold text-slate-300 hover:border-mint hover:text-mint disabled:text-slate-600"
        data-testid="live-check"
        disabled={props.liveChecking || props.liveActive}
        onClick={() => props.onCheckLive?.()}
      >
        {props.liveActive ? "LIVE OPEN" : props.liveChecking ? "CHECKING LIVE" : "OPEN LIVE"}
      </button>

      <Show when={sessionActionStatus(ingestState())}>
        {(message) => <span class="text-amber">{message()}</span>}
      </Show>
      <Show when={props.liveActive}>
        <span class="text-mint">Live race owns the dashboard.</span>
      </Show>
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
      <Show when={props.liveStatusMessage}>
        {(message) => <span class="text-slate-400">{message()}</span>}
      </Show>
    </div>
  );
}
