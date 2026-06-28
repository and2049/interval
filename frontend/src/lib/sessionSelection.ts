import type { Meeting, Season, Session, SessionReadiness } from "../../../shared/types/api";
import { sessionStatusLabel } from "./sessionReadiness";

export interface SessionSelectOption {
  value: number;
  label: string;
  disabled?: boolean;
  title?: string;
}

export function nextSelection<T>(
  options: T[] | undefined,
  selected: number | undefined,
  keyFor: (item: T) => number,
  preferred?: number
): number | undefined {
  if (!options) return selected;
  if (options.length === 0) return undefined;
  if (selected != null && options.some((item) => keyFor(item) === selected)) return selected;
  if (preferred != null && options.some((item) => keyFor(item) === preferred)) return preferred;
  return keyFor(options[0]);
}

export function nextSeasonSelection(
  seasons: { year: number }[] | undefined,
  selected: number | undefined,
  preferred?: number
) {
  return nextSelection(seasons, selected, (season) => season.year, preferred);
}

export function nextMeetingSelection(
  meetings: Meeting[] | undefined,
  selected: number | undefined,
  preferred?: number
) {
  return nextSelection(meetings, selected, (meeting) => meeting.meeting_key, preferred);
}

export function nextSessionSelection(
  sessions: SessionReadiness[] | undefined,
  selected: number | undefined,
  preferred?: number
) {
  const selectable = sessions?.filter((entry) => entry.support_status === "supported");
  return nextSelection(selectable, selected, (entry) => entry.session.session_key, preferred);
}

export function readinessForSession(
  sessions: SessionReadiness[] | undefined,
  selected: number | undefined
) {
  if (selected == null) return undefined;
  return sessions?.find((entry) => entry.session.session_key === selected);
}

export function activeSessionSelectionMatches(
  active: Session,
  selected: {
    season?: number;
    meeting?: number;
    session?: number;
  }
) {
  return (
    selected.season === active.year &&
    selected.meeting === active.meeting_key &&
    selected.session === active.session_key
  );
}

export function shouldSyncActiveSessionSelection(
  active: Session,
  selected: {
    season?: number;
    meeting?: number;
    session?: number;
  },
  lastActiveSessionKey?: number
) {
  if (active.session_key !== lastActiveSessionKey) return true;
  return (
    selected.session === active.session_key &&
    !activeSessionSelectionMatches(active, selected)
  );
}

export function seasonOptions(seasons: Season[] | undefined): SessionSelectOption[] {
  return (seasons ?? []).map((season) => ({
    value: season.year,
    label: season.year.toString()
  }));
}

export function meetingOptions(meetings: Meeting[] | undefined): SessionSelectOption[] {
  return (meetings ?? []).map((meeting) => ({
    value: meeting.meeting_key,
    label: meeting.name
  }));
}

export function sessionOptions(sessions: SessionReadiness[] | undefined): SessionSelectOption[] {
  return (sessions ?? []).map((entry) => ({
    value: entry.session.session_key,
    label: `${sessionDisplayName(entry.session)} · ${sessionStatusLabel(entry)}`,
    disabled: entry.support_status !== "supported",
    title: entry.support_reason ?? undefined
  }));
}

export function selectedSessionLabel(entry: SessionReadiness | undefined): string | undefined {
  if (!entry) return undefined;
  return `${entry.session.year} ${sessionDisplayName(entry.session)} #${entry.session.session_key}`;
}

export function sessionTypeLabel(session: Session): string {
  return session.session_type === "sprint" ? "SPRINT" : "RACE";
}

export function sessionDisplayName(session: Session): string {
  const typeLabel = sessionTypeLabel(session);
  return session.name.trim().toLowerCase() === session.session_type
    ? typeLabel
    : `${typeLabel} ${session.name}`;
}
