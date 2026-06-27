import type {
  IngestStatus,
  Meeting,
  IngestResponse,
  ReplayMetadata,
  ReplaySnapshot,
  ReplayEventListResponse,
  Season,
  SessionReadiness,
  TrackGeometry
} from "../../../shared/types/api";

const INGEST_STATUSES = new Set<IngestStatus>([
  "not_ingested",
  "fetching",
  "normalizing",
  "ready",
  "failed"
]);

const json = async <T>(url: string): Promise<T> => {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(await apiErrorMessage(response));
  }
  return response.json() as Promise<T>;
};

const postIngest = async (url: string): Promise<IngestResponse> => {
  const response = await fetch(url, { method: "POST" });
  return ingestResponseOrThrow(response);
};

export async function ingestResponseOrThrow(response: Response): Promise<IngestResponse> {
  const body = await response.text();
  const parsed = parseJsonObject(body);
  if (isIngestResponse(parsed)) return parsed;

  if (!response.ok) {
    throw new Error(apiErrorMessageFromBody(response.status, response.statusText, body));
  }
  throw new Error("Invalid ingest response.");
}

export async function apiErrorMessage(response: Response): Promise<string> {
  return apiErrorMessageFromBody(response.status, response.statusText, await response.text());
}

function apiErrorMessageFromBody(status: number, statusText: string, body: string): string {
  const fallback = `${status} ${statusText}`.trim();
  if (!body.trim()) return fallback;

  try {
    const payload = JSON.parse(body) as { error?: unknown; message?: unknown };
    if (typeof payload.error === "string" && payload.error.trim()) return payload.error;
    if (typeof payload.message === "string" && payload.message.trim()) return payload.message;
  } catch {
    return body.trim();
  }

  return fallback;
}

function parseJsonObject(body: string): unknown {
  if (!body.trim()) return undefined;
  try {
    return JSON.parse(body);
  } catch {
    return undefined;
  }
}

function isIngestResponse(value: unknown): value is IngestResponse {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<IngestResponse>;
  return (
    typeof candidate.session_key === "number" &&
    isIngestStatus(candidate.status) &&
    typeof candidate.cached_endpoints === "number" &&
    Array.isArray(candidate.endpoint_coverage) &&
    typeof candidate.generated_snapshots === "number" &&
    Array.isArray(candidate.warnings) &&
    Object.prototype.hasOwnProperty.call(candidate, "track_geometry") &&
    Object.prototype.hasOwnProperty.call(candidate, "available_channels") &&
    Object.prototype.hasOwnProperty.call(candidate, "error")
  );
}

function isIngestStatus(value: unknown): value is IngestStatus {
  return typeof value === "string" && INGEST_STATUSES.has(value as IngestStatus);
}

function positiveInteger(value: number, label: string): number {
  if (Number.isInteger(value) && value > 0) return value;
  throw new Error(`${label} must be a positive integer.`);
}

function finiteNumber(value: number, label: string): number {
  if (Number.isFinite(value)) return value;
  throw new Error(`${label} must be a finite number.`);
}

export const api = {
  seasons: () => json<Season[]>("/api/seasons"),
  meetings: (season: number) =>
    json<Meeting[]>(`/api/meetings?season=${positiveInteger(season, "season")}`),
  sessions: (meetingKey: number) =>
    json<SessionReadiness[]>(
      `/api/sessions?meeting_key=${positiveInteger(meetingKey, "meeting key")}`
    ),
  ingest: (sessionKey: number) =>
    postIngest(`/api/sessions/${positiveInteger(sessionKey, "session key")}/ingest`),
  metadata: (sessionKey: number) =>
    json<ReplayMetadata>(
      `/api/sessions/${positiveInteger(sessionKey, "session key")}/replay/metadata`
    ),
  snapshot: (sessionKey: number, t: number) =>
    json<ReplaySnapshot>(
      `/api/sessions/${positiveInteger(
        sessionKey,
        "session key"
      )}/replay/snapshot?t=${finiteNumber(t, "replay time").toFixed(3)}`
    ),
  streamUrl: (sessionKey: number, from: number, speed: number) =>
    `/api/sessions/${positiveInteger(
      sessionKey,
      "session key"
    )}/replay/stream?from=${finiteNumber(from, "replay start time").toFixed(
      3
    )}&speed=${finiteNumber(speed, "replay speed").toFixed(3)}`,
  events: (sessionKey: number) =>
    json<ReplayEventListResponse>(
      `/api/sessions/${positiveInteger(sessionKey, "session key")}/replay/events`
    ),
  trackGeometry: (sessionKey: number) =>
    json<TrackGeometry>(
      `/api/sessions/${positiveInteger(sessionKey, "session key")}/track/geometry`
    )
};
