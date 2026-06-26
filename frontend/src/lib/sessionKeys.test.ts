import { afterEach, describe, expect, test } from "bun:test";
import { LAST_SESSION_STORAGE_KEY, readStoredSessionKey, writeStoredSessionKey } from "./sessionKeys";

const originalLocalStorage = globalThis.localStorage;

afterEach(() => {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: originalLocalStorage
  });
});

describe("session key storage", () => {
  test("reads valid stored session ids", () => {
    installStorage({ [LAST_SESSION_STORAGE_KEY]: "9472" });

    expect(readStoredSessionKey()).toBe(9472);
  });

  test("ignores missing, invalid, and non-positive values", () => {
    installStorage({});
    expect(readStoredSessionKey()).toBeUndefined();

    installStorage({ [LAST_SESSION_STORAGE_KEY]: "not-a-number" });
    expect(readStoredSessionKey()).toBeUndefined();

    installStorage({ [LAST_SESSION_STORAGE_KEY]: "-1" });
    expect(readStoredSessionKey()).toBeUndefined();
  });

  test("writes the selected session id", () => {
    const storage = installStorage({});

    writeStoredSessionKey(9839);

    expect(storage[LAST_SESSION_STORAGE_KEY]).toBe("9839");
  });
});

function installStorage(initial: Record<string, string>) {
  const storage = { ...initial };
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => storage[key] ?? null,
      setItem: (key: string, value: string) => {
        storage[key] = value;
      }
    }
  });
  return storage;
}
