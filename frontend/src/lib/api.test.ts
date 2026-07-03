import { describe, expect, test } from "bun:test";
import { api, apiErrorMessage, ingestResponseOrThrow } from "./api";

describe("apiErrorMessage", () => {
  test("uses backend error fields from JSON responses", async () => {
    const response = new Response(JSON.stringify({ error: "resource not found" }), {
      status: 404,
      statusText: "Not Found"
    });

    await expect(apiErrorMessage(response)).resolves.toBe("resource not found");
  });

  test("uses message fields when error is absent", async () => {
    const response = new Response(JSON.stringify({ message: "rate limited" }), {
      status: 429,
      statusText: "Too Many Requests"
    });

    await expect(apiErrorMessage(response)).resolves.toBe("rate limited");
  });

  test("preserves plain-text response bodies", async () => {
    const response = new Response("upstream unavailable", {
      status: 502,
      statusText: "Bad Gateway"
    });

    await expect(apiErrorMessage(response)).resolves.toBe("upstream unavailable");
  });

  test("falls back to status text for empty or unhelpful bodies", async () => {
    await expect(
      apiErrorMessage(new Response("", { status: 500, statusText: "Internal Server Error" }))
    ).resolves.toBe("500 Internal Server Error");

    await expect(
      apiErrorMessage(new Response(JSON.stringify({ error: "" }), { status: 400, statusText: "Bad Request" }))
    ).resolves.toBe("400 Bad Request");
  });
});

describe("ingestResponseOrThrow", () => {
  test("preserves structured failed ingest envelopes from non-2xx responses", async () => {
    const response = new Response(JSON.stringify(ingestResponse({ status: "failed" })), {
      status: 502,
      statusText: "Bad Gateway"
    });

    await expect(ingestResponseOrThrow(response)).resolves.toMatchObject({
      session_key: 9472,
      status: "failed",
      error: "OpenF1 unavailable"
    });
  });

  test("throws transport errors when the ingest body is not a typed envelope", async () => {
    const response = new Response(JSON.stringify({ error: "proxy failed" }), {
      status: 502,
      statusText: "Bad Gateway"
    });

    await expect(ingestResponseOrThrow(response)).rejects.toThrow("proxy failed");
  });

  test("rejects malformed successful ingest responses", async () => {
    const response = new Response(JSON.stringify({ ok: true }), {
      status: 200,
      statusText: "OK"
    });

    await expect(ingestResponseOrThrow(response)).rejects.toThrow("Invalid ingest response.");
  });

  test("rejects ingest envelopes with unknown status values", async () => {
    const response = new Response(
      JSON.stringify({ ...ingestResponse(), status: "half_cached" }),
      {
        status: 200,
        statusText: "OK"
      }
    );

    await expect(ingestResponseOrThrow(response)).rejects.toThrow("Invalid ingest response.");
  });
});

describe("api parameter validation", () => {
  test("rejects invalid numeric ids before calling fetch", async () => {
    await withMockFetch(async (calls) => {
      expect(() => api.meetings(Number.NaN)).toThrow("season must be a positive integer.");
      expect(() => api.sessions(0)).toThrow("meeting key must be a positive integer.");
      expect(() => api.metadata(12.5)).toThrow("session key must be a positive integer.");
      expect(() => api.ingest(-1)).toThrow("session key must be a positive integer.");
      expect(calls).toEqual([]);
    });
  });

  test("rejects invalid replay times before calling fetch", async () => {
    await withMockFetch(async (calls) => {
      expect(() => api.snapshot(9472, Number.NaN)).toThrow("replay time must be a finite number.");
      expect(calls).toEqual([]);
    });
  });

  test("formats valid snapshot requests with fixed precision", async () => {
    await withMockFetch(async (calls) => {
      await api.snapshot(9472, 12.3456);
      expect(calls).toEqual(["/api/sessions/9472/replay/snapshot?t=12.346"]);
    });
  });

  test("formats stream urls with start time and playback speed", () => {
    expect(api.streamUrl(9472, 12.3456, 2)).toBe(
      "/api/sessions/9472/replay/stream?from=12.346&speed=2.000"
    );
  });

  test("formats live stream urls", () => {
    expect(api.liveStreamUrl(9472)).toBe("/api/sessions/9472/live/stream");
  });

  test("formats live session lifecycle requests", async () => {
    await withMockFetch(async (calls) => {
      await api.liveCurrent();
      await api.liveStart(9472);
      await api.liveStatus(9472);
      await api.liveMetadata(9472);
      await api.liveSnapshot(9472);
      await api.liveEvents(9472);
      await api.liveTrackGeometry(9472);
      await api.liveStop(9472);
      for (const call of [
        () => api.liveStart(0),
        () => api.liveStatus(0),
        () => api.liveMetadata(0),
        () => api.liveSnapshot(0),
        () => api.liveEvents(0),
        () => api.liveTrackGeometry(0),
        () => api.liveStop(0)
      ]) {
        expect(call).toThrow("session key must be a positive integer.");
      }
      expect(calls).toEqual([
        "/api/live/current",
        "/api/sessions/9472/live/start",
        "/api/sessions/9472/live/status",
        "/api/sessions/9472/live/metadata",
        "/api/sessions/9472/live/snapshot",
        "/api/sessions/9472/live/events",
        "/api/sessions/9472/live/track/geometry",
        "/api/sessions/9472/live/stop"
      ]);
    });
  });

});

function ingestResponse(overrides: { status?: "ready" | "failed" } = {}) {
  const status = overrides.status ?? "ready";
  return {
    session_key: 9472,
    status,
    cached_endpoints: status === "ready" ? 11 : 0,
    endpoint_coverage: [],
    generated_snapshots: status === "ready" ? 1200 : 0,
    track_geometry: null,
    available_channels: null,
    warnings: [],
    error: status === "failed" ? "OpenF1 unavailable" : null
  };
}

async function withMockFetch(run: (calls: string[]) => Promise<void>) {
  const originalFetch = globalThis.fetch;
  const calls: string[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    calls.push(input.toString());
    return new Response(JSON.stringify({}), { status: 200, statusText: "OK" });
  }) as typeof fetch;

  try {
    await run(calls);
  } finally {
    globalThis.fetch = originalFetch;
  }
}
