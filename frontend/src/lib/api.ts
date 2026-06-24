import type {
  Meeting,
  IngestResponse,
  ReplayMetadata,
  ReplaySnapshot,
  ReplayEventListResponse,
  Season,
  SessionReadiness,
  TrackGeometry
} from "../../../shared/types/api";

const json = async <T>(url: string): Promise<T> => {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
};

const postJson = async <T>(url: string): Promise<T> => {
  const response = await fetch(url, { method: "POST" });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
};

export const api = {
  seasons: () => json<Season[]>("/api/seasons"),
  meetings: (season: number) => json<Meeting[]>(`/api/meetings?season=${season}`),
  sessions: (meetingKey: number) =>
    json<SessionReadiness[]>(`/api/sessions?meeting_key=${meetingKey}`),
  ingest: (sessionKey: number) =>
    postJson<IngestResponse>(`/api/sessions/${sessionKey}/ingest`),
  metadata: (sessionKey: number) =>
    json<ReplayMetadata>(`/api/sessions/${sessionKey}/replay/metadata`),
  snapshot: (sessionKey: number, t: number) =>
    json<ReplaySnapshot>(`/api/sessions/${sessionKey}/replay/snapshot?t=${t.toFixed(3)}`),
  events: (sessionKey: number) =>
    json<ReplayEventListResponse>(`/api/sessions/${sessionKey}/replay/events`),
  trackGeometry: (sessionKey: number) =>
    json<TrackGeometry>(`/api/sessions/${sessionKey}/track/geometry`)
};
