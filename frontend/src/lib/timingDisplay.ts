import type { DriverSnapshot, Sector } from "../../../shared/types/api";

const SECTOR_COUNT = 3;

export function sectorCells(sectors: Sector[]): Array<Sector | undefined> {
  return Array.from({ length: SECTOR_COUNT }, (_, index) => sectors[index]);
}

export function hasTimingRows(rows: DriverSnapshot[]): boolean {
  return rows.length > 0;
}

export function gapLabel(position: number, gap?: string | null): string {
  if (position === 1) return "LEADER";
  return gap ?? "--";
}

export function intervalLabel(interval?: string | null): string {
  return interval ?? "--";
}
