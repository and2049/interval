import type { IngestResponse, IngestStatus, SessionReadiness } from "../../../shared/types/api";

export type SessionActionState = "idle" | "ingesting" | "failed";
export type IngestOutcomeTone = "ready" | "degraded" | "failed";

export interface IngestOutcome {
  label: string;
  tone: IngestOutcomeTone;
  title?: string;
}

export function sessionStatusLabel(entry: SessionReadiness) {
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
  return entry.is_demo ? "DEMO" : entry.ingest_status.replace("_", " ").toUpperCase();
}

export function sessionActionLabel(args: {
  ingestState: SessionActionState;
  selectedSession?: number;
  activeSessionKey: number;
  readiness?: SessionReadiness;
}) {
  if (args.ingestState === "ingesting") return "INGESTING";
  if (args.selectedSession == null) return "SELECT SESSION";
  if (args.readiness?.replay_ready || args.readiness?.is_demo) {
    return args.selectedSession === args.activeSessionKey ? "RELOAD CACHE" : "OPEN CACHE";
  }
  return "INGEST + OPEN";
}

export function canOpenSessionFromCache(readiness?: SessionReadiness) {
  return Boolean(readiness?.replay_ready || readiness?.is_demo);
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
    args.ingestState !== "ingesting"
  );
}

export function sessionIngestErrorMessage(args: {
  ingestError?: string;
  readiness?: SessionReadiness;
}) {
  return args.ingestError ?? args.readiness?.last_error ?? "Ingest failed.";
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
