import type { EventKind, EventSeverity, ReplayEvent } from "../../../shared/types/api";

export type EventFeedState = "ready" | "loading" | "error" | "empty";

export function recentReplayEvents(events: ReplayEvent[], t: number, limit = 6): ReplayEvent[] {
  const cursor = Number.isFinite(t) ? t : 0;
  return events
    .filter((event) => event.t <= cursor)
    .sort((a, b) => b.t - a.t)
    .slice(0, Math.max(0, limit));
}

export function eventKindLabel(kind: EventKind): string {
  return kind.replaceAll("_", " ").toUpperCase();
}

export function eventSeverityClass(severity: EventSeverity): string {
  switch (severity) {
    case "critical":
      return "text-danger";
    case "warning":
      return "text-amber";
    case "notice":
      return "text-mint";
    default:
      return "text-slate-300";
  }
}

export function eventFeedState(args: {
  rows: ReplayEvent[];
  loading: boolean;
  error?: unknown;
}): EventFeedState {
  if (args.rows.length > 0) return "ready";
  if (args.loading) return "loading";
  if (args.error) return "error";
  return "empty";
}

export function eventFeedEmptyLabel(state: EventFeedState): string {
  switch (state) {
    case "loading":
      return "Loading replay events...";
    case "error":
      return "Replay event feed unavailable";
    case "empty":
      return "No replay events yet";
    case "ready":
      return "";
  }
}
