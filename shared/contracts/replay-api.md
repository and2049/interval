# Replay API Contract

The backend owns historical ingest, normalization, caching, replay timeline lookup, live polling, and derived metrics. FastF1 is the primary historical replay source; OpenF1 remains available for discovery, legacy cached rebuilds, and live race data. The frontend owns playback controls and renders versioned replay contracts. MVP contract version is `replay.v1`.

## Endpoints

### `GET /api/seasons`

Returns cached seasons.

### `GET /api/meetings?season=YYYY`

Returns cached race meetings for a season.

### `GET /api/sessions?meeting_key=...`

Returns race and sprint sessions for the selected meeting with replay readiness. MVP scope excludes practice, qualifying, sprint qualifying, and sprint shootout.

```json
[
  {
    "session": {
      "session_key": 9472,
      "meeting_key": 1229,
      "year": 2024,
      "name": "Race",
      "session_type": "race",
      "start_time": "2024-03-02T15:00:00Z",
      "end_time": "2024-03-02T17:00:00Z",
      "total_laps": 57
    },
    "ingest_status": "not_ingested",
    "replay_ready": false,
    "is_demo": false,
    "last_error": null
  }
]
```

### `POST /api/sessions/{session_key}/ingest`

Fetches the historical FastF1 bundle for a race or sprint session, stores raw sections in SQLite, normalizes records, generates replay snapshots, and persists replay metadata/events. The backend resolves OpenF1 meeting/session metadata to a FastF1 round and session code, using curated overrides only for known special cases such as Bahrain `9472`.

Response:

```json
{
  "session_key": 9839,
  "cached_endpoints": 11,
  "generated_snapshots": 1440,
  "status": "ready",
  "endpoint_coverage": [
    { "endpoint": "drivers", "present": true, "rows": 20 }
  ],
  "track_geometry": {
    "status": "ready",
    "source": "open_f1_location",
    "quality": "ready"
  },
  "available_channels": {
    "timing": true,
    "location": false,
    "track_geometry": true,
    "weather": true,
    "race_control": true,
    "stints": true,
    "pit_events": true,
    "intervals": true
  },
  "warnings": [],
  "error": null
}
```

`warnings` can include degraded channel coverage and resolver notes, for example approximate FastF1 schedule matches, curated override usage, or a summary such as `FastF1 resolved Japanese Grand Prix to round 4 via fastf1_schedule_match. confidence 0.917`.

Failure responses keep the same envelope so the frontend can show the error while session readiness records the failed ingest state. FastF1 fetch/runtime failures return `502`, fetch timeouts return `504`, and replay rebuild failures return `500` with `"status": "failed"` and an `"error"` message.

The curated fixture is the 2024 Bahrain Grand Prix race (`session_key=9472`). It remains a seeded known-good example and FastF1 resolver override, not the only supported historical replay. The frontend should not fall back to Bahrain when another selected replay is missing; selecting a cached race or sprint should open it, and selecting an uncached historical race or sprint should start ingest for that selected session while keeping it visible. The seeded Abu Dhabi fixture (`session_key=9839`) remains an offline demo and is marked `is_demo: true` in session readiness.

### `GET /api/sessions/{session_key}/replay/metadata`

Returns session metadata, optional meeting identity, replay duration, frame bounds, drivers, data sources, available channels, geometry status, and endpoint links.

```json
{
  "contract_version": "replay.v1",
  "session": {
    "session_key": 9472,
    "meeting_key": 1229,
    "year": 2024,
    "name": "Race",
    "session_type": "race"
  },
  "meeting": {
    "meeting_key": 1229,
    "year": 2024,
    "name": "Bahrain Grand Prix",
    "country": "Bahrain",
    "location": "Sakhir"
  },
  "duration_seconds": 5996.749,
  "frame_step_seconds": 0.2,
  "available_channels": {
    "timing": true,
    "location": true,
    "track_geometry": true,
    "weather": true,
    "race_control": true,
    "stints": true,
    "pit_events": true,
    "intervals": true
  },
  "track_geometry": {
    "status": "ready",
    "source": "curated_static",
    "quality": "ready"
  }
}
```

### `GET /api/sessions/{session_key}/replay/snapshot?t=...`

Returns the deterministic replay snapshot at or immediately before `t`. Requests before the first frame clamp to the first frame; requests after the last frame clamp to the final frame. The snapshot is the synchronized contract for all panels.

Clients may animate their own playback cursor, but should request snapshots on persisted frame boundaries using `ReplayMetadata.frame_step_seconds`. Repeated requests inside the same frame window are expected to resolve to the same backend snapshot.

FastF1 historical replays currently use 5 Hz persisted snapshots (`0.2s`). Legacy OpenF1/demo replays can report a different cadence through the same field.

```json
{
  "contract_version": "replay.v1",
  "cursor": { "session_key": 9472, "t": 600.0, "frame_index": 120 },
  "race_state": { "lap": 4, "track_status": "green" },
  "timing": {
    "quality": "ready",
    "rows": [
      {
        "position": 1,
        "rank_source": "open_f1_position",
        "gap_to_leader": null,
        "interval": null
      }
    ]
  },
  "track": {
    "map_mode": "projected",
    "quality": "projected",
    "positions": [
      {
        "driver_number": 1,
        "x": 123.0,
        "y": 456.0,
        "relative_distance": 0.42,
        "source": "projected",
        "quality": "projected"
      }
    ]
  }
}
```

`track.quality` describes the position data used by the map for that snapshot. For the Bahrain MVP fallback, geometry can be ready while map positions are still `projected`.

`race_control.messages` in a snapshot is a recent panel window for the current replay time, capped to keep high-cadence snapshots lightweight. Use `GET /api/sessions/{session_key}/replay/events` for the complete canonical race-control and derived event timeline.

`derived_metrics` is backend-owned snapshot data, not a frontend calculation surface. MVP metrics include recent 3-lap pace values for drivers with enough completed valid laps:

```json
{
  "driver_number": 1,
  "label": "3-lap avg",
  "value": "96.936",
  "trend": "stable"
}
```

### `GET /api/sessions/{session_key}/replay/events`

Returns a versioned event list ordered by replay time. Event kinds include `race_control`, `track_status`, `pit_stop`, `stint_change`, `leader_change`, `weather_change`, and `data_gap`.

`race_control`, `track_status`, and `pit_stop` originate from normalized historical records, usually FastF1 for newly ingested historical races and OpenF1 for legacy cached replays. `stint_change`, `leader_change`, and `weather_change` are backend-derived from normalized stints, position records, and weather samples. Derived events use `"source": "derived"` and remain synchronized to replay time `t`.

Returns `404` when replay artifacts do not exist for the session. A valid replay with no timeline events returns `200` with an empty `events` array.

```json
{
  "contract_version": "replay.v1",
  "events": [
    {
      "id": "race-control-125.000-4be81a2c",
      "t": 120.0,
      "kind": "race_control",
      "severity": "warning",
      "driver_number": null,
      "message": "Yellow flag in sector 2.",
      "source": "open_f1",
      "payload": {}
    }
  ]
}
```

### `GET /api/sessions/{session_key}/replay/stream`

SSE endpoint that emits the same v1 metadata, snapshot, and event shapes used by REST. Event names are `metadata`, `snapshot`, `event`, `end`, and `error`. Snapshot lookup remains the source of truth, and snapshot events use the nested v1 shape without legacy top-level `drivers` or `positions` fields.

### Live Simulation

Live simulation is an in-memory backend mode for testing live behavior from an already cached historical replay. It does not call OpenF1 live endpoints. FastF1 remains historical-only; OpenF1 is the real live source.

- `POST /api/sessions/{session_key}/live-simulation/start`
- `GET /api/sessions/{session_key}/live-simulation/status`
- `GET /api/sessions/{session_key}/live-simulation/snapshot`
- `GET /api/sessions/{session_key}/live-simulation/stream`
- `POST /api/sessions/{session_key}/live-simulation/stop`

The live stream emits the same `metadata`, `snapshot`, `event`, `end`, and `error` SSE event names as replay streaming. Snapshot payloads remain `ReplaySnapshot` with `contract_version: "replay.v1"`. Metadata emitted by the live stream labels `data_sources[0].name` as `"live_simulation"` so the frontend can show a live-simulation badge without changing panel data contracts.

Live simulation starts from `ReplayMetadata.race_start_t` when that value is present and inside replay bounds, so pre-grid and formation-lap time is skipped for normal testing. If `race_start_t` is unavailable, the simulator falls back to `min_t`.

### OpenF1 Live

OpenF1 live mode is enabled by default. Disable it for offline/dev runs with `INTERVAL_OPENF1_LIVE_ENABLED=false`, `0`, `no`, or `off`. Optional configuration includes `INTERVAL_OPENF1_LIVE_BASE_URL`, `INTERVAL_OPENF1_LIVE_TOKEN`, and `INTERVAL_OPENF1_LIVE_AUTH_HEADER`. With the default `Authorization` header, `INTERVAL_OPENF1_LIVE_TOKEN` accepts either a raw token such as `abc123` or a prefixed value such as `Bearer abc123`.

- `GET /api/live/current`
- `POST /api/sessions/{session_key}/live/start`
- `GET /api/sessions/{session_key}/live/status`
- `GET /api/sessions/{session_key}/live/metadata`
- `GET /api/sessions/{session_key}/live/snapshot`
- `GET /api/sessions/{session_key}/live/events`
- `GET /api/sessions/{session_key}/live/stream`
- `GET /api/sessions/{session_key}/live/track/geometry`
- `POST /api/sessions/{session_key}/live/stop`

The backend detects an active race or sprint from OpenF1 session metadata once the session start time has been reached, with post-session padding kept open for reconnects. Pre-session races are reported as `next_session`, not `active`, so the frontend waits instead of opening live before timing/location rows exist. Starting live mode fetches the latest OpenF1 rows, normalizes them into the same internal race model, and exposes `ReplayMetadata` plus `ReplaySnapshot` with `data_sources[0].name = "openf1_live"`. The frontend checks `/api/live/current` on startup, polls inactive/error availability with a backed-off cadence, keeps OpenF1 row-warmup checks responsive, and auto-connects when a live session is available.

`/api/live/current` returns a typed `availability` value: `disabled`, `inactive`, `active`, or `error`. If the backend already has a running in-memory live session, this endpoint returns that active session and its `status` before doing fresh OpenF1 discovery, which keeps browser reloads resilient during transient discovery failures. If OpenF1 discovery finds an active race or sprint that has not been started in the backend yet, the response has `active: true` and `status: null`; the frontend then calls `/live/start` to create the runtime snapshot state. When no session is active but a future race or sprint exists in the current OpenF1 season payload, inactive responses may include `next_session` and `next_meeting` so the dashboard can show what live session it is waiting for. Discovery failures are reported as `{ "availability": "error", "active": false, "message": "..." }` rather than as a failed app startup path, so the dashboard can stay usable for historical replay while making live status visible. The frontend calls this endpoint on startup, polls it periodically while availability remains `inactive` or retryable, and also exposes a manual `OPEN LIVE` action so a user can open a live race after the app has already loaded.

OpenF1 polling is cadenced by channel inside the backend live runtime. High-motion channels (`position`, `location`) refresh at roughly `0.5s`; interval/timing support channels refresh around `1-2s`; static or slower channels (`drivers`, `stints`, `weather`, `session_result`) are cached longer and reused between live snapshots. Refresh work is serialized and coalesced per runtime, so additional SSE or REST readers consume the same current snapshot instead of multiplying upstream polling. HTTP requests are paced and have a bounded timeout. Failed endpoints retry with exponential backoff while preserving their last successful payload.

If live start is inside the session window but OpenF1 has not published usable driver/timing/location rows yet, `/live/start` returns `503 Service Unavailable` with the initial snapshot message. The frontend treats this as a waiting/retry state, not a fatal live configuration failure. If a required initial OpenF1 channel fails at the HTTP/API layer, `/live/start` returns `502 Bad Gateway` so operators can distinguish upstream failure from normal pre-row warmup.

`/live/status` includes `source: "openf1_live"`, `updated_at`, and `channels`, a compact endpoint health list with `endpoint`, `state`, `age_seconds`, `rows`, and optional `last_error`. State values are `fresh`, `cached`, `stale`, `missing`, and `failed`. Cached data with a refresh error becomes `stale` once its last successful fetch exceeds the channel freshness window; a non-empty payload is not considered healthy indefinitely. Optional event-like feeds such as `pit`, `race_control`, and `session_result` may be `fresh` with `rows: 0`; this means OpenF1 responded successfully and no rows are currently expected, not that the channel is broken. If a refresh receives malformed live rows after a session is already running, the backend keeps the last good snapshot visible and adds a synthetic `refresh` channel with `state: "failed"` and `last_error` explaining the normalization failure. The frontend uses these values for live endpoint badges while keeping all dashboard panels driven by `ReplaySnapshot`.

For real-session validation, `scripts/check-live-current.ps1 -Strict` asserts critical-channel health, `/live/status.updated_at` freshness, live snapshot clock sync to the OpenF1 session clock, and that the current snapshot has enough timing/map rows to drive the core dashboard panels. Strict mode expands to `-FailOnBadChannels -MaxUpdateAgeSeconds 15 -MaxSnapshotClockLagSeconds 30 -MinTimingRows 10 -MinTrackPositions 10`, while allowing degradable feeds such as intervals, pit, race-control, stints, weather, and session-result to be bad without failing when the core snapshot is healthy. Pass `-AllowedBadChannels @()` to make every bad channel fail. Add `-MinGeometryPoints 20` once cached, historical, or accumulated live location geometry is expected. Add `-MinEvents 1` once race-control, pit, weather, or derived events are expected. Before the session opens, use `-ExpectedNextMeetingName`, `-ExpectedNextSessionType`, and/or `-ExpectedNextSessionKey` to verify OpenF1 is advertising the correct next live race or sprint, or add `-WaitForActiveSeconds` with `-Start -RequireActive -Strict` to wait until the session becomes active and then validate. During the active window, add `-ExpectedMeetingName`, `-ExpectedSessionType`, and/or `-ExpectedSessionKey` to fail fast when OpenF1 reports a different live session than the one being validated.

`/live/metadata` returns the current in-memory live `ReplayMetadata` response directly. This mirrors historical replay metadata lookup and gives reconnecting or freshly opened clients a stable metadata payload before the SSE stream's `metadata` event arrives. Live metadata uses `frame_step_seconds: 0.5`, `data_sources[0].name: "openf1_live"`, and live endpoint links:

```json
{
  "snapshot_endpoint": "/api/sessions/{session_key}/live/snapshot",
  "stream_endpoint": "/api/sessions/{session_key}/live/stream",
  "events_endpoint": "/api/sessions/{session_key}/live/events",
  "track_geometry_endpoint": "/api/sessions/{session_key}/live/track/geometry"
}
```

Live track geometry is selected by the backend. The live runtime first uses geometry already stored for the active live session, then reuses ready geometry from a cached historical session with the same meeting country/location, and only then derives geometry from incoming OpenF1 `location` rows or falls back to schematic geometry. Reused historical geometry is returned with the live `session_key`, so frontend rendering remains source-agnostic and can treat it like any other `TrackGeometry`.

`/live/events` returns the current in-memory live event timeline as a `ReplayEventListResponse`. This gives reconnecting clients and freshly opened dashboards an immediate event feed without waiting for the next SSE event window.

`/live/stream` emits typed `event` frames in addition to `metadata` and `snapshot`. Live event payloads use the same `ReplayEvent` shape as replay events and are generated from normalized race-control, pit, weather, gap, and driver-status records. A new or reconnected stream re-emits the current event history; clients deduplicate by event `id`, which recovers events that arrived while disconnected without a second live payload model. During the post-session reconnect window, the snapshot cursor continues to follow wall-clock session time instead of freezing at the scheduled end.

The frontend treats real live as a first-class dashboard mode: scrub and speed controls are disabled, streamed `event` frames populate the event feed, endpoint health badges come from `/live/status`, and SSE disconnects use explicit exponential backoff reconnect attempts while keeping the latest good snapshot visible. Async snapshot/start results and SSE handlers are generation-owned, so an obsolete replay request or closed stream cannot overwrite or terminate a newer live mode. Superseded live and live-simulation starts also stop any backend runtime they created.

### `GET /api/sessions/{session_key}/track/geometry`

Returns static track geometry for the selected session.

```json
{
  "contract_version": "replay.v1",
  "session_key": 9472,
  "source": "curated_static",
  "quality": "ready",
  "map_mode": "projected",
  "bounds": { "min_x": 0.0, "max_x": 100.0, "min_y": 0.0, "max_y": 100.0 },
  "centerline": []
}
```

For FastF1 historical replays, FastF1 telemetry geometry is preferred. Legacy OpenF1 replays still prefer OpenF1 `location` when available, then curated static geometry for known fixtures such as Bahrain, then schematic fallback. Timing order remains based on normalized position/interval records, not map projection.
