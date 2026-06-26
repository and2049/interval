export function parseSelectNumber(value: string): number | undefined {
  if (!value.trim()) return undefined;

  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}
