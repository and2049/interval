import type { ReplayMetadata, ReplaySnapshot, TrackGeometry } from "../../../shared/types/api";

const ALLOWED_SPEEDS = [0.5, 1, 2, 4] as const;

export function clampReplayTime(t: number, maxT: number): number {
  if (!Number.isFinite(t)) return 0;
  if (!Number.isFinite(maxT) || maxT <= 0) return Math.max(0, t);
  return Math.min(maxT, Math.max(0, t));
}

export function advanceReplayTime(current: number, elapsedSeconds: number, speed: number, maxT: number): number {
  if (!Number.isFinite(elapsedSeconds) || elapsedSeconds <= 0) {
    return clampReplayTime(current, maxT);
  }
  return clampReplayTime(current + elapsedSeconds * normalizeReplaySpeed(speed), maxT);
}

export function nextReplayTick(options: {
  currentTime: number;
  elapsedSeconds: number;
  speed: number;
  maxT?: number;
  playing: boolean;
}) {
  if (!options.playing || options.maxT == null) {
    return { time: options.currentTime, playing: options.playing };
  }

  const time = advanceReplayTime(
    options.currentTime,
    options.elapsedSeconds,
    options.speed,
    options.maxT
  );

  return {
    time,
    playing: time < options.maxT
  };
}

export function normalizeReplaySpeed(speed: number): number {
  return ALLOWED_SPEEDS.includes(speed as (typeof ALLOWED_SPEEDS)[number]) ? speed : 1;
}

export function parseReplayTimeInput(value: string): number | undefined {
  if (!value.trim()) return undefined;

  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

export function parseReplaySpeedInput(value: string): number {
  const parsed = parseReplayTimeInput(value);
  return normalizeReplaySpeed(parsed ?? 1);
}

export function quantizeReplayFrameTime(t: number, metadata: ReplayMetadata): number {
  const clamped = clampReplayTime(t, metadata.max_t);
  const step = metadata.frame_step_seconds;
  if (!Number.isFinite(step) || step <= 0) return clamped;

  const minT = Number.isFinite(metadata.min_t) ? metadata.min_t : 0;
  if (clamped <= minT) return minT;

  const frameIndex = Math.floor((clamped - minT) / step);
  return clampReplayTime(minT + frameIndex * step, metadata.max_t);
}

export function shouldReloadSession(currentSessionKey: number, nextSessionKey: number): boolean {
  return currentSessionKey === nextSessionKey;
}

export function replayResourceSessionKey(
  currentSessionKey: number,
  metadata?: ReplayMetadata
): number | undefined {
  return metadata?.session.session_key === currentSessionKey ? currentSessionKey : undefined;
}

export function snapshotRequest(
  currentSessionKey: number,
  metadata: ReplayMetadata | undefined,
  t: number
): { key: number; t: number } | undefined {
  const key = replayResourceSessionKey(currentSessionKey, metadata);
  return key === undefined || metadata === undefined
    ? undefined
    : { key, t: quantizeReplayFrameTime(t, metadata) };
}

export function activeReplayMetadata(
  currentSessionKey: number,
  metadata?: ReplayMetadata
): ReplayMetadata | undefined {
  return metadata?.session.session_key === currentSessionKey ? metadata : undefined;
}

export function activeReplaySnapshot(
  currentSessionKey: number,
  snapshot?: ReplaySnapshot
): ReplaySnapshot | undefined {
  return snapshot?.cursor.session_key === currentSessionKey ? snapshot : undefined;
}

export function activeTrackGeometry(
  currentSessionKey: number,
  geometry?: TrackGeometry
): TrackGeometry | undefined {
  return geometry?.session_key === currentSessionKey ? geometry : undefined;
}

export function activeResourceError(
  currentSessionKey: number,
  resourceSessionKey: number | undefined,
  error?: unknown
): unknown {
  return resourceSessionKey === currentSessionKey ? error : undefined;
}

export function activeResourceLoading(
  currentSessionKey: number,
  resourceSessionKey: number | undefined,
  loading: boolean
): boolean {
  return resourceSessionKey === currentSessionKey && loading;
}

export function replayLoadMessage(options: {
  metadata?: ReplayMetadata;
  metadataLoading?: boolean;
  metadataError?: unknown;
  snapshotLoading?: boolean;
  snapshotError?: unknown;
  sessionKey?: number;
  preferredHistoricalSessionKey?: number;
}): string {
  if (options.metadataLoading && !options.metadata) return "Connecting to replay cache...";
  if (options.snapshotLoading && options.metadata) return "Loading replay frame...";
  if (options.metadataError) {
    if (isMissingReplay(options.metadataError)) {
      return options.sessionKey === options.preferredHistoricalSessionKey
        ? "Bahrain replay is not cached yet. Choose INGEST + OPEN to fetch FastF1 data."
        : "Replay is not cached yet. Choose INGEST + OPEN for this session.";
    }
    return errorText(options.metadataError, "Replay metadata unavailable.");
  }
  if (options.snapshotError) return errorText(options.snapshotError, "Replay snapshot unavailable.");
  if (options.metadata) return "Loading replay frame...";
  return "Connecting to replay cache...";
}

export function replaySessionTitle(metadata: ReplayMetadata): string {
  const sessionName = metadata.session.name.trim() || `Session ${metadata.session.session_key}`;
  const meetingName = metadata.meeting?.name?.trim();
  return meetingName
    ? `${metadata.session.year} ${meetingName} · ${sessionName}`
    : `${metadata.session.year} ${sessionName} · #${metadata.session.session_key}`;
}

function isMissingReplay(error: unknown): boolean {
  const text = errorText(error, "").toLowerCase();
  return text.includes("resource not found") || text.includes("404");
}

function errorText(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}
