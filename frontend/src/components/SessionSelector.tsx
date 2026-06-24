import { createEffect, createResource, createSignal, For, Show } from "solid-js";
import type { IngestResponse, IngestStatus, SessionReadiness } from "../../../shared/types/api";
import { api } from "../lib/api";

interface SessionSelectorProps {
  activeSessionKey: number;
  onOpenSession: (sessionKey: number) => void;
}

export function SessionSelector(props: SessionSelectorProps) {
  const [selectedSeason, setSelectedSeason] = createSignal<number>();
  const [selectedMeeting, setSelectedMeeting] = createSignal<number>();
  const [selectedSession, setSelectedSession] = createSignal<number>();
  const [ingestState, setIngestState] = createSignal<"idle" | "ingesting" | "failed">("idle");
  const [lastIngest, setLastIngest] = createSignal<IngestResponse>();

  const [seasons] = createResource(api.seasons);
  const [meetings] = createResource(selectedSeason, api.meetings);
  const [sessions] = createResource(selectedMeeting, api.sessions);

  createEffect(() => {
    const firstSeason = seasons()?.[0]?.year;
    if (selectedSeason() == null && firstSeason != null) setSelectedSeason(firstSeason);
  });

  createEffect(() => {
    const firstMeeting = meetings()?.[0]?.meeting_key;
    if (firstMeeting != null) setSelectedMeeting(firstMeeting);
  });

  createEffect(() => {
    const firstSession = sessions()?.[0]?.session.session_key;
    if (firstSession != null) setSelectedSession(firstSession);
  });

  const selectedReadiness = () =>
    sessions()?.find((entry) => entry.session.session_key === selectedSession());

  const openSelected = async () => {
    const key = selectedSession();
    if (key == null) return;
    setIngestState("ingesting");
    try {
      const response = await api.ingest(key);
      setLastIngest(response);
      props.onOpenSession(key);
      setIngestState("idle");
    } catch {
      setIngestState("failed");
    }
  };

  return (
    <div class="flex items-center gap-2 border-b border-line bg-[#101419] px-3 py-2 font-mono text-[0.72rem]">
      <label class="text-slate-500">Season</label>
      <select
        class="border border-line bg-panel px-2 py-1 text-slate-100"
        value={selectedSeason()}
        disabled={seasons.loading}
        onChange={(event) => setSelectedSeason(Number(event.currentTarget.value))}
      >
        <For each={seasons() ?? []}>
          {(season) => <option value={season.year}>{season.year}</option>}
        </For>
      </select>

      <label class="ml-2 text-slate-500">Meeting</label>
      <select
        class="max-w-64 border border-line bg-panel px-2 py-1 text-slate-100"
        value={selectedMeeting()}
        disabled={meetings.loading}
        onChange={(event) => setSelectedMeeting(Number(event.currentTarget.value))}
      >
        <For each={meetings() ?? []}>
          {(meeting) => <option value={meeting.meeting_key}>{meeting.name}</option>}
        </For>
      </select>

      <label class="ml-2 text-slate-500">Session</label>
      <select
        class="border border-line bg-panel px-2 py-1 text-slate-100"
        value={selectedSession()}
        disabled={sessions.loading}
        onChange={(event) => setSelectedSession(Number(event.currentTarget.value))}
      >
        <For each={sessions() ?? []}>
          {(entry: SessionReadiness) => (
            <option value={entry.session.session_key}>
              {entry.session.name} · {statusLabel(entry)}
            </option>
          )}
        </For>
      </select>

      <Show when={selectedReadiness()}>
        {(entry) => (
          <span class={`border px-2 py-1 ${statusClass(entry().ingest_status)}`}>
            {entry().is_demo ? "DEMO" : entry().ingest_status.replace("_", " ").toUpperCase()}
          </span>
        )}
      </Show>

      <button
        class="ml-2 border border-mint bg-mint/10 px-3 py-1 font-semibold text-mint disabled:border-line disabled:text-slate-500"
        disabled={selectedSession() == null || ingestState() === "ingesting"}
        onClick={openSelected}
      >
        {ingestState() === "ingesting" ? "INGESTING" : selectedSession() === props.activeSessionKey ? "RELOAD" : "INGEST + OPEN"}
      </button>

      <Show when={ingestState() === "failed"}>
        <span class="text-danger">
          Ingest failed{selectedReadiness()?.last_error ? `: ${selectedReadiness()?.last_error}` : "."}
        </span>
      </Show>
      <Show when={lastIngest()?.warnings.length}>
        <span class="max-w-[28rem] truncate text-amber">
          {lastIngest()!.warnings[0]}
        </span>
      </Show>
      <Show when={meetings.error || sessions.error}>
        <span class="text-danger">OpenF1 discovery failed.</span>
      </Show>
    </div>
  );
}

function statusLabel(entry: SessionReadiness) {
  if (entry.is_demo) return "demo";
  if (entry.replay_ready) return "ready";
  return entry.ingest_status.replace("_", " ");
}

function statusClass(status: IngestStatus) {
  switch (status) {
    case "ready":
      return "border-mint text-mint";
    case "failed":
      return "border-danger text-danger";
    case "fetching":
    case "normalizing":
      return "border-amber text-amber";
    default:
      return "border-line text-slate-400";
  }
}
