import type {
  LiveAvailability,
  Meeting,
  ReplayMetadata,
  ReplaySnapshot,
  Session,
  TrackGeometry
} from "../../../shared/types/api";

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

export function shouldReloadSession(
  currentSessionKey: number | undefined,
  nextSessionKey: number
): boolean {
  return currentSessionKey === nextSessionKey;
}

export function replayResourceSessionKey(
  currentSessionKey: number | undefined,
  metadata?: ReplayMetadata
): number | undefined {
  if (currentSessionKey == null) return undefined;
  return metadata?.session.session_key === currentSessionKey ? currentSessionKey : undefined;
}

export function snapshotRequest(
  currentSessionKey: number | undefined,
  metadata: ReplayMetadata | undefined,
  t: number
): { key: number; t: number } | undefined {
  const key = replayResourceSessionKey(currentSessionKey, metadata);
  return key === undefined || metadata === undefined
    ? undefined
    : { key, t: quantizeReplayFrameTime(t, metadata) };
}

export function activeReplayMetadata(
  currentSessionKey: number | undefined,
  metadata?: ReplayMetadata
): ReplayMetadata | undefined {
  if (currentSessionKey == null) return undefined;
  return metadata?.session.session_key === currentSessionKey ? metadata : undefined;
}

export function activeReplaySnapshot(
  currentSessionKey: number | undefined,
  snapshot?: ReplaySnapshot
): ReplaySnapshot | undefined {
  if (currentSessionKey == null) return undefined;
  return snapshot?.cursor.session_key === currentSessionKey ? snapshot : undefined;
}

export function activeTrackGeometry(
  currentSessionKey: number | undefined,
  geometry?: TrackGeometry
): TrackGeometry | undefined {
  if (currentSessionKey == null) return undefined;
  return geometry?.session_key === currentSessionKey ? geometry : undefined;
}

export function activeResourceError(
  currentSessionKey: number | undefined,
  resourceSessionKey: number | undefined,
  error?: unknown
): unknown {
  if (currentSessionKey == null) return undefined;
  return resourceSessionKey === currentSessionKey ? error : undefined;
}

export function activeResourceLoading(
  currentSessionKey: number | undefined,
  resourceSessionKey: number | undefined,
  loading: boolean
): boolean {
  if (currentSessionKey == null) return false;
  return resourceSessionKey === currentSessionKey && loading;
}

export function replayLoadMessage(options: {
  metadata?: ReplayMetadata;
  metadataLoading?: boolean;
  metadataError?: unknown;
  snapshotLoading?: boolean;
  snapshotError?: unknown;
  sessionKey?: number;
  selectedSessionLabel?: string;
  liveStatusMessage?: string;
  liveConnecting?: boolean;
}): string {
  if (options.liveConnecting) {
    return options.liveStatusMessage?.trim() || "Connecting to live session...";
  }
  if (options.metadataLoading && !options.metadata) return "Connecting to replay cache...";
  if (options.snapshotLoading && options.metadata) return "Loading replay frame...";
  if (options.metadataError) {
    if (isMissingReplay(options.metadataError)) {
      return options.selectedSessionLabel
        ? `No cached replay for selected race: ${options.selectedSessionLabel}. Ingest starts automatically when supported.`
        : "Replay is not cached yet. Select a supported race or sprint to ingest it.";
    }
    return errorText(options.metadataError, "Replay metadata unavailable.");
  }
  if (options.snapshotError) return errorText(options.snapshotError, "Replay snapshot unavailable.");
  if (options.metadata) return "Loading replay frame...";
  if (options.sessionKey == null) {
    return options.selectedSessionLabel
      ? `No cached replay for selected race: ${options.selectedSessionLabel}. Ingest starts automatically when supported.`
      : options.liveStatusMessage?.trim()
        || "Live races open automatically when available. Select a historical race or sprint to replay.";
  }
  return "Connecting to replay cache...";
}

export function shouldClearMissingHistoricalReplay(options: {
  metadataError?: unknown;
  metadataLoading?: boolean;
  sessionKey?: number;
  liveActive?: boolean;
  liveSimulationActive?: boolean;
}): boolean {
  return Boolean(
    options.sessionKey != null
      && !options.metadataLoading
      && !options.liveActive
      && !options.liveSimulationActive
      && options.metadataError
      && isMissingReplay(options.metadataError)
  );
}

export function shouldApplyLiveStartResult(
  requestId: number,
  latestRequestId: number
): boolean {
  return requestId === latestRequestId;
}

export function shouldApplySnapshotResult(
  requestId: number,
  latestRequestId: number,
  requestedSessionKey: number,
  currentSessionKey: number | undefined,
  liveActive: boolean,
  liveSimulationActive: boolean,
  liveTransitioning = false
): boolean {
  return requestId === latestRequestId
    && requestedSessionKey === currentSessionKey
    && !liveActive
    && !liveSimulationActive
    && !liveTransitioning;
}

export function shouldApplyLiveResourceResult(
  resourceSessionKey: number | undefined,
  currentSessionKey: number | undefined,
  liveActive: boolean
): boolean {
  return liveActive && resourceSessionKey != null && resourceSessionKey === currentSessionKey;
}

export function shouldHideHistoricalResourceError(liveActive: boolean): boolean {
  return liveActive;
}

export function openF1LiveSessionKeyToStop(
  currentSessionKey: number | undefined,
  liveActive: boolean
): number | undefined {
  return liveActive && currentSessionKey != null ? currentSessionKey : undefined;
}

export function liveSimulationSessionKeyToStop(
  currentSessionKey: number | undefined,
  liveSimulationActive: boolean
): number | undefined {
  return liveSimulationActive && currentSessionKey != null ? currentSessionKey : undefined;
}

export function sessionKeyAfterLiveStops(
  liveSessionKey: number | undefined,
  returnSessionKey: number | undefined
): number | undefined {
  return returnSessionKey != null && returnSessionKey !== liveSessionKey
    ? returnSessionKey
    : undefined;
}

export function replaySessionTitle(metadata: ReplayMetadata): string {
  const sessionName = metadata.session.name.trim() || `Session ${metadata.session.session_key}`;
  const meetingName = metadata.meeting?.name?.trim();
  return meetingName
    ? `${metadata.session.year} ${meetingName} · ${sessionName}`
    : `${metadata.session.year} ${sessionName} · #${metadata.session.session_key}`;
}

export function serverSentErrorMessage(event: Event): string | undefined {
  const data = (event as MessageEvent).data;
  return typeof data === "string" && data.trim() ? data : undefined;
}

export function liveCurrentMessage(
  availability: LiveAvailability,
  message?: string | null,
  nextSession?: Session | null,
  nextMeeting?: Meeting | null
): string | undefined {
  switch (availability) {
    case "disabled":
      return message?.trim() || "LIVE disabled";
    case "inactive":
      if (nextSession) {
        const meeting = nextMeeting?.name?.trim();
        const session = nextSession.name.trim() || `Session ${nextSession.session_key}`;
        const start = liveSessionStartLabel(nextSession.start_time);
        return meeting
          ? `Next live: ${nextSession.year} ${meeting} · ${session}${start}`
          : `Next live: ${nextSession.year} ${session}${start}`;
      }
      return message?.trim() || "No active live race or sprint";
    case "error":
      return message?.trim() || "OpenF1 live status unavailable";
    default:
      return undefined;
  }
}

export function liveCheckErrorMessage(error: unknown): string {
  return errorText(error, "OpenF1 live status unavailable.");
}

export function liveStartErrorMessage(error: unknown): string {
  const text = errorText(error, "OpenF1 live session could not be opened.");
  return isWaitingForOpenF1LiveDataError(error)
    ? `Waiting for OpenF1 live data. ${text}`
    : text;
}

export function isWaitingForOpenF1LiveDataError(error: unknown): boolean {
  return errorText(error, "").includes("OpenF1 live initial snapshot has no");
}

export function liveAvailabilityAfterStartError(error: unknown): LiveAvailability {
  return isWaitingForOpenF1LiveDataError(error) ? "inactive" : "error";
}

export function shouldPollLiveAvailability(availability: LiveAvailability): boolean {
  return availability === "inactive" || availability === "error";
}

export function liveAvailabilityPollDelayMs(
  availability: LiveAvailability,
  message?: string
): number {
  if (availability === "inactive" && message?.startsWith("Waiting for OpenF1 live data.")) {
    return 10_000;
  }
  if (availability === "inactive" || availability === "error") return 60_000;
  return Number.POSITIVE_INFINITY;
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

function liveSessionStartLabel(startTime: string): string {
  const parsed = Date.parse(startTime);
  if (!Number.isFinite(parsed)) return "";
  const date = new Date(parsed);
  const yyyy = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  const hh = String(date.getUTCHours()).padStart(2, "0");
  const mm = String(date.getUTCMinutes()).padStart(2, "0");
  return ` · ${yyyy}-${month}-${day} ${hh}:${mm} UTC`;
}
