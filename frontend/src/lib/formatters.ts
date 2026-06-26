import type { SectorStatus, TyreCompound } from "../../../shared/types/api";

export function formatRaceClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  return formatClockParts(total);
}

export function formatEventClock(seconds: number): string {
  const total = Math.floor(seconds);
  if (total < 0) return `T-${formatClockParts(Math.abs(total))}`;
  return formatClockParts(total);
}

function formatClockParts(total: number): string {
  const mins = Math.floor(total / 60).toString().padStart(2, "0");
  const secs = (total % 60).toString().padStart(2, "0");
  return `${mins}:${secs}`;
}

export function formatLapTime(value?: number | null): string {
  if (value == null || Number.isNaN(value)) return "--";
  const minutes = Math.floor(value / 60);
  const seconds = value - minutes * 60;
  return `${minutes}:${seconds.toFixed(3).padStart(6, "0")}`;
}

export function formatTemperature(value?: number | null): string {
  if (value == null || Number.isNaN(value)) return "--";
  return `${value.toFixed(1)}C`;
}

export function formatPercent(value?: number | null): string {
  if (value == null || Number.isNaN(value)) return "--";
  return `${value.toFixed(0)}%`;
}

export function formatSpeed(value?: number | null): string {
  if (value == null || Number.isNaN(value)) return "--";
  return `${value.toFixed(1)} m/s`;
}

export function sectorClass(status: SectorStatus): string {
  if (status === "overall_best") return "text-fuchsia-300";
  if (status === "personal_best") return "text-mint";
  if (status === "normal") return "text-timing";
  return "text-slate-500";
}

export function compoundClass(compound: TyreCompound): string {
  switch (compound) {
    case "SOFT":
      return "border-danger text-danger";
    case "MEDIUM":
      return "border-amber text-amber";
    case "HARD":
      return "border-slate-100 text-slate-100";
    case "INTERMEDIATE":
      return "border-emerald-400 text-emerald-300";
    case "WET":
      return "border-sky-400 text-sky-300";
    default:
      return "border-slate-500 text-slate-400";
  }
}

export function compoundAbbreviation(compound: TyreCompound): string {
  if (compound === "INTERMEDIATE") return "I";
  if (compound === "UNKNOWN") return "--";
  return compound[0] ?? "--";
}

export function trendClass(trend: string): string {
  if (trend === "improving") return "text-mint";
  if (trend === "degrading") return "text-danger";
  return "text-slate-300";
}
