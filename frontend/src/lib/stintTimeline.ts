export interface StintProgressDisplay {
  label: string;
  widthPercent: number;
  known: boolean;
}

export function stintProgressDisplay(stintAge?: number | null): StintProgressDisplay {
  if (stintAge == null || !Number.isFinite(stintAge) || stintAge < 0) {
    return {
      label: "Age --",
      widthPercent: 0,
      known: false
    };
  }

  return {
    label: `Age ${Math.floor(stintAge)}`,
    widthPercent: Math.min(100, Math.max(0, stintAge * 4)),
    known: true
  };
}
