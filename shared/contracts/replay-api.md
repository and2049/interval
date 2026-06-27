# Replay API Contract

The backend owns historical ingest, normalization, caching, replay timeline lookup, and derived metrics. FastF1 is the primary historical replay source; OpenF1 remains available for discovery, legacy cached rebuilds, and future live data. The frontend owns playback controls and renders versioned replay contracts. MVP contract version is `replay.v1`.

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
      "id": "race-control-0",
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
