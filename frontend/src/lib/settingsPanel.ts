import type { OpenF1TokenProbe, OpenF1TokenSettings } from "../../../shared/types/api";
import type { ChannelBadge } from "./replayQuality";

export function isSubmittableToken(value: string): boolean {
  return value.trim().length > 0;
}

/** One line describing what the backend is currently using, and where it came from. */
export function tokenSourceLine(settings: OpenF1TokenSettings): string {
  switch (settings.source) {
    case "settings":
      return `Saved token ${settings.hint ?? ""}`.trim();
    case "env":
      return "Using INTERVAL_OPENF1_LIVE_TOKEN from the environment";
    default:
      return "No token configured";
  }
}

/**
 * Shown only when a saved token is shadowing an environment one. Without this the user
 * edits `.env`, sees nothing change, and has no way to find out why.
 */
export function envOverrideNotice(settings: OpenF1TokenSettings): string | undefined {
  if (settings.source !== "settings" || !settings.env_token_present) return undefined;
  return "INTERVAL_OPENF1_LIVE_TOKEN is also set. The token saved here takes precedence; clear it to use the environment value.";
}

export function probeBadge(probe: OpenF1TokenProbe): ChannelBadge {
  switch (probe.result) {
    case "ok":
      return { label: "TOKEN OK", ready: true, tone: "ready" };
    case "unauthorized":
      return { label: "REJECTED", ready: false, tone: "missing" };
    case "unreachable":
      return { label: "UNREACHABLE", ready: false, tone: "missing" };
    default:
      return { label: "INVALID", ready: false, tone: "degraded" };
  }
}

export function saveErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return "Could not save the token.";
}
