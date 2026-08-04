import { describe, expect, test } from "bun:test";
import type { OpenF1TokenProbe, OpenF1TokenSettings } from "../../../shared/types/api";
import {
  envOverrideNotice,
  isSubmittableToken,
  probeBadge,
  saveErrorMessage,
  tokenSourceLine
} from "./settingsPanel";

describe("isSubmittableToken", () => {
  test("accepts a non-blank token", () => {
    expect(isSubmittableToken("abc")).toBe(true);
  });

  test("rejects blank input", () => {
    expect(isSubmittableToken("")).toBe(false);
    expect(isSubmittableToken("   ")).toBe(false);
  });
});

describe("tokenSourceLine", () => {
  test("shows the masked hint for a saved token", () => {
    expect(tokenSourceLine(settings({ source: "settings", hint: "••••n123" }))).toBe(
      "Saved token ••••n123"
    );
  });

  test("names the environment variable when that is what is in use", () => {
    expect(tokenSourceLine(settings({ source: "env" }))).toBe(
      "Using INTERVAL_OPENF1_LIVE_TOKEN from the environment"
    );
  });

  test("reports when nothing is configured", () => {
    expect(tokenSourceLine(settings({ source: "none", configured: false }))).toBe(
      "No token configured"
    );
  });
});

describe("envOverrideNotice", () => {
  test("warns only when a saved token is shadowing an environment one", () => {
    expect(
      envOverrideNotice(settings({ source: "settings", env_token_present: true }))
    ).toContain("takes precedence");
  });

  test("stays silent when there is nothing being shadowed", () => {
    expect(
      envOverrideNotice(settings({ source: "settings", env_token_present: false }))
    ).toBeUndefined();
    expect(
      envOverrideNotice(settings({ source: "env", env_token_present: true }))
    ).toBeUndefined();
    expect(
      envOverrideNotice(settings({ source: "none", env_token_present: false }))
    ).toBeUndefined();
  });
});

describe("probeBadge", () => {
  test("maps each probe result to a distinct tone", () => {
    expect(probeBadge(probe("ok")).tone).toBe("ready");
    expect(probeBadge(probe("unauthorized")).tone).toBe("missing");
    expect(probeBadge(probe("unreachable")).tone).toBe("missing");
    expect(probeBadge(probe("invalid")).tone).toBe("degraded");
  });

  test("distinguishes a rejected token from an unreachable api", () => {
    expect(probeBadge(probe("unauthorized")).label).toBe("REJECTED");
    expect(probeBadge(probe("unreachable")).label).toBe("UNREACHABLE");
  });
});

describe("saveErrorMessage", () => {
  test("prefers the server's message", () => {
    expect(saveErrorMessage(new Error("token must not be blank"))).toBe(
      "token must not be blank"
    );
    expect(saveErrorMessage("plain failure")).toBe("plain failure");
  });

  test("falls back when there is nothing useful to show", () => {
    expect(saveErrorMessage(new Error("   "))).toBe("Could not save the token.");
    expect(saveErrorMessage(undefined)).toBe("Could not save the token.");
  });
});

function settings(overrides: Partial<OpenF1TokenSettings> = {}): OpenF1TokenSettings {
  return {
    configured: true,
    hint: "••••abcd",
    source: "settings",
    env_token_present: false,
    path: "/config/interval/settings.json",
    ...overrides
  };
}

function probe(result: OpenF1TokenProbe["result"]): OpenF1TokenProbe {
  return { result, message: "message" };
}
