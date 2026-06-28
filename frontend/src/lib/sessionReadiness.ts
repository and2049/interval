import type { IngestResponse, IngestStatus, SessionReadiness } from "../../../shared/types/api";

export type SessionActionState =
  | "idle"
  | "checking"
  | "opening_cache"
  | "ingesting"
  | "opening_replay"
  | "failed";
export type IngestOutcomeTone = "ready" | "degraded" | "failed";

export interface IngestOutcome {
  label: string;
  tone: IngestOutcomeTone;
  title?: string;
}

export function sessionStatusLabel(entry: SessionReadiness) {
  if (entry.support_status === "cancelled") return "cancelled";
  if (entry.support_status === "future") return "not available yet";
  if (entry.is_demo) return "demo";
  if (entry.replay_ready) return "ready";
  return entry.ingest_status.replace("_", " ");
}

export function sessionStatusClass(status: IngestStatus) {
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

export function sessionStatusBadgeText(entry: SessionReadiness) {
  if (entry.support_status === "cancelled") return "CANCELLED";
  if (entry.support_status === "future") return "FUTURE";
  return entry.is_demo ? "DEMO" : entry.ingest_status.replace("_", " ").toUpperCase();
}

export function sessionActionLabel(args: {
  ingestState: SessionActionState;
  selectedSession?: number;
  activeSessionKey?: number;
  readiness?: SessionReadiness;
}) {
  if (args.readiness && !isSessionSupported(args.readiness)) return "UNAVAILABLE";
  if (args.ingestState === "checking") return "CHECKING";
  if (args.ingestState === "opening_cache" || args.ingestState === "opening_replay") {
    return "OPENING";
  }
  if (args.ingestState === "ingesting") return "INGESTING";
  if (args.ingestState === "failed") return "RETRY";
  if (args.selectedSession == null) return "SELECT SESSION";
  if (args.readiness?.replay_ready || args.readiness?.is_demo) {
    return args.selectedSession === args.activeSessionKey ? "RELOAD" : "OPEN CACHE";
  }
  return "INGEST + OPEN";
}

export function isSessionSupported(readiness?: SessionReadiness) {
  return !readiness || readiness.support_status === "supported";
}

export function canStartSessionAction(readiness?: SessionReadiness) {
  return Boolean(readiness && isSessionSupported(readiness));
}

export function canOpenSessionFromCache(readiness?: SessionReadiness) {
  return Boolean(isSessionSupported(readiness) && (readiness?.replay_ready || readiness?.is_demo));
}

export function canOpenSessionAfterIngest(response: IngestResponse) {
  return response.status === "ready" && response.generated_snapshots > 0;
}

export function shouldClearTransientSessionAction(args: {
  previousSession?: number;
  selectedSession?: number;
  ingestState: SessionActionState;
}) {
  return (
    args.previousSession !== args.selectedSession &&
    args.previousSession != null &&
    !isBusySessionAction(args.ingestState)
  );
}

export function isBusySessionAction(state: SessionActionState) {
  return (
    state === "checking" ||
    state === "opening_cache" ||
    state === "ingesting" ||
    state === "opening_replay"
  );
}

export function sessionActionStatus(state: SessionActionState): string | undefined {
  switch (state) {
    case "checking":
      return "Checking selected replay...";
    case "opening_cache":
      return "Opening cached replay...";
    case "ingesting":
      return "Ingesting selected session...";
    case "opening_replay":
      return "Opening replay...";
    case "failed":
    case "idle":
      return undefined;
  }
}

export function sessionIngestErrorMessage(args: {
  ingestError?: string;
  readiness?: SessionReadiness;
}) {
  return (
    args.ingestError ??
    args.readiness?.support_reason ??
    args.readiness?.last_error ??
    "Ingest failed."
  );
}

export function ingestOutcome(response?: IngestResponse): IngestOutcome | undefined {
  if (!response) return undefined;
  if (response.status === "failed") {
    return {
      label: response.error ?? "Ingest failed.",
      tone: "failed"
    };
  }

  const frameLabel =
    response.generated_snapshots === 1
      ? "1 frame"
      : `${response.generated_snapshots.toLocaleString()} frames`;
  if (response.warnings.length === 0) {
    return {
      label: `Cached ${frameLabel}`,
      tone: "ready"
    };
  }

  const warningLabel =
    response.warnings.length === 1
      ? "1 warning"
      : `${response.warnings.length} warnings`;
  return {
    label: `Cached ${frameLabel} · ${warningLabel}`,
    tone: "degraded",
    title: response.warnings.join(" | ")
  };
}

export function ingestOutcomeClass(tone: IngestOutcomeTone) {
  switch (tone) {
    case "ready":
      return "text-mint";
    case "degraded":
      return "text-amber";
    case "failed":
      return "text-danger";
  }
}
