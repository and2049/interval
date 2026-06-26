export const DEMO_SESSION_KEY = 9839;
export const MVP_HISTORICAL_SESSION_KEY = 9472;
export const MVP_HISTORICAL_SEASON = 2024;
export const MVP_HISTORICAL_MEETING_KEY = 1229;
export const LAST_SESSION_STORAGE_KEY = "interval:last-session-key";

export function readStoredSessionKey(): number | undefined {
  try {
    const value = globalThis.localStorage?.getItem(LAST_SESSION_STORAGE_KEY);
    if (!value) return undefined;
    const parsed = Number(value);
    return Number.isInteger(parsed) && parsed > 0 ? parsed : undefined;
  } catch {
    return undefined;
  }
}

export function writeStoredSessionKey(sessionKey: number) {
  try {
    globalThis.localStorage?.setItem(LAST_SESSION_STORAGE_KEY, String(sessionKey));
  } catch {
    // Local storage can be unavailable in restricted browser modes.
  }
}
