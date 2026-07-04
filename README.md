# Interval

Live and replay F1 second-screen dashboard for race and sprint sessions.

The app uses one backend-owned `replay.v1` snapshot contract for OpenF1-backed live races, deterministic live simulation, and FastF1 historical replay cached in SQLite. The 2024 Bahrain Grand Prix race (`session_key=9472`) remains the seeded example and resolver override.

## Current Shape

- `backend/`: Rust API service with Axum, SQLite, FastF1 historical ingest, OpenF1 discovery/live polling, replay generation, track geometry, and SSE streaming.
- `frontend/`: SolidJS, TypeScript, Vite, and Tailwind dashboard.
- `shared/`: public replay API contract docs and TypeScript wire types.
- `infra/`: early deployment notes.
- `docs/`: local reference notes and inspiration material; this directory is ignored by Git in this workspace.

The reference `docs/f1-race-replay` project is used for modeling ideas only. This project does not port its Python/UI stack.

## Historical Replay Path

The frontend opens the last selected cached session when available. If that replay is missing or stale, the app clears the active replay and leaves the selector in control instead of falling back to Bahrain. Selecting a race meeting or session is active intent: cached replays open automatically, and uncached historical race or sprint sessions start FastF1 ingest automatically before opening. The seeded Bahrain race (`session_key=9472`) remains available as a known-good FastF1 override, and the seeded demo replay (`session_key=9839`) remains available for offline UI development.

For historical replays, FastF1 telemetry is the preferred source for driver locations and track geometry. Race and sprint ingest resolves OpenF1 meeting/session metadata to a FastF1 year, round, and session code, with curated overrides for known special cases. For older cached OpenF1 data, the backend still:

- prefers usable upstream location geometry when present;
- falls back to curated Bahrain geometry for `session_key=9472`;
- projects driver dots onto that centerline for the map;
- keeps timing order based on normalized position/interval data, not map projection.

Replay payloads use `contract_version: "replay.v1"`. REST snapshots and SSE snapshots share the same nested shape. FastF1 historical replays currently persist frames at 5 Hz (`frame_step_seconds = 0.2`); legacy OpenF1/demo replays may use a lower cadence. The frontend advances a local smooth cursor for playback controls, but snapshot requests are quantized to the backend `frame_step_seconds` so the dashboard does not refetch the same persisted frame on every animation tick.

Backend-derived snapshot sections currently include 3-lap pace metrics, map quality labels, race-control history, weather context, timing rows, and projected track positions. Panels should read those sections directly instead of inferring state from raw OpenF1 records.

## Run Locally

Backend:

```bash
cargo run -p interval-backend
```

The backend listens on `http://127.0.0.1:4000` and uses `sqlite://interval.db` unless `DATABASE_URL` is set.

Frontend:

```bash
cd frontend
bun install
bun run dev
```

The Vite dev server proxies `/api` to the backend.

Useful checks:

```bash
curl http://127.0.0.1:4000/healthz
curl http://127.0.0.1:4000/api/sessions/9472/replay/metadata
curl "http://127.0.0.1:4000/api/sessions/9472/replay/snapshot?t=600"
curl http://127.0.0.1:4000/api/sessions/9472/track/geometry
```

If Bahrain has not been ingested in the local database, select it in the app; the frontend will start ingest automatically. The manual selector button remains available for retry/reload.

## Runtime Configuration

Backend environment variables:

- `DATABASE_URL`: SQLite URL, defaults to `sqlite://interval.db`.
- `INTERVAL_BIND`: backend bind address, defaults to `127.0.0.1:4000`.
- `INTERVAL_REBUILD_SESSION_ON_START`: optional session key to rebuild replay artifacts from cached raw historical data during backend startup. The smoke script uses `9472`.
- `INTERVAL_FASTF1_PYTHON`: optional Python executable with FastF1 dependencies already installed. If omitted, the backend creates `cache/fastf1-venv` and installs `scripts/fastf1-requirements.txt` on first FastF1 ingest.
- `INTERVAL_FASTF1_BOOTSTRAP_PYTHON`: optional Python executable used to create the managed FastF1 venv. Defaults to `python`.
- `INTERVAL_OPENF1_LIVE_ENABLED`: OpenF1 live discovery and polling are enabled by default; set to `false`, `0`, `no`, or `off` to disable them for offline/dev runs.
- `INTERVAL_OPENF1_LIVE_BASE_URL`: optional OpenF1-compatible live API base URL, defaults to `https://api.openf1.org/v1/`.
- `INTERVAL_OPENF1_LIVE_TOKEN`: optional server-side live API token. With the default `Authorization` header, either `abc123` or `Bearer abc123` is accepted.
- `INTERVAL_OPENF1_LIVE_AUTH_HEADER`: optional auth header name for the token, defaults to `Authorization`.

For local live testing, copy `.env.example` to `.env` and put the OpenF1 sponsor token there. The backend loads `.env` on startup, and shell-provided environment variables still override it.
- `RUST_LOG`: tracing filter, defaults to `interval_backend=info,tower_http=info`.

With OpenF1 live enabled, the frontend checks `GET /api/live/current` on startup, polls quietly while no live session is active, and keeps warmup checks responsive while OpenF1 rows are starting to publish. The selector bar shows live availability as `LIVE CHECKING`, `LIVE WAITING`, `LIVE READY`, `LIVE OPEN`, `LIVE OFF`, or `LIVE ERROR`, and the control bar exposes `OPEN LIVE` for a manual check/start. A race or sprint becomes active at its OpenF1 session start time; before that it is shown as the next live session so the app does not open live before timing/location rows exist. Once active, the backend starts an in-memory live session, polls OpenF1 channels on endpoint-specific cadences, and streams the same `ReplaySnapshot` shape used by historical replay. Reloading the app during a running backend live session reconnects through that in-memory session even if upstream discovery has a transient failure. Individual OpenF1 live HTTP requests are bounded by a request timeout so one slow endpoint cannot freeze live refresh indefinitely. `LIVE SIM` remains available as a deterministic test mode from cached historical sessions.

Smoke script parameters:

- `-DatabaseUrl`: SQLite URL passed to the backend process.
- `-BackendBind`: backend bind address used by the smoke backend.
- `-FrontendPort`: Vite port used by the smoke frontend.
- `-TimeoutSeconds`: health-check and stream-read timeout.
- `-SessionKey`, `-MeetingKey`, `-ExpectedMeetingName`, and `-SnapshotT`: cached replay target. Defaults cover Bahrain `9472`.
- `-ExpectedMapMode`, `-ExpectedTrackQuality`, `-ExpectedGeometrySource`, and `-MinimumTimingRows`: target-specific replay quality assertions. Defaults match the Bahrain curated/projected fallback.

## Verification

Run the current gates before treating a change as stable:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-static.ps1
```

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1
```

The static check parses the PowerShell scripts. The main verification script runs that static check, backend tests, frontend tests, the frontend production build, and the OpenF1 live smoke gate against a local mock.
The script checks for required local tools (`cargo` and `bun`) before running the gates.

After a historical race has been ingested once into `interval.db`, run the full cached MVP verification. With no extra parameters this checks the seeded Bahrain target:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -WithSmoke
```

The smoke step builds a fresh backend binary, rebuilds the requested replay artifacts from the cached raw historical bundle, starts the backend and Vite dev server, confirms the selected replay is cached, then checks replay metadata, meeting context, a mid-race snapshot with section quality labels and 3-lap derived metrics, invalid replay-cursor handling, track geometry, REST/proxied replay events, initial SSE stream events, metadata-driven stream cadence, and the Vite API proxy before shutting the local smoke processes down.

Live mode is part of the default verification gate and does not require real OpenF1 credentials. It starts local OpenF1-compatible mock servers, points the backend at them, starts the backend and Vite proxy, then checks pre-session waiting, post-session reconnect clamping, warmup `503` retry behavior, required-channel `502` failure behavior, malformed refresh fallback with a synthetic failed `refresh` status channel, live discovery, start, metadata, mid-session snapshot sync, status, event feed, track geometry, typed SSE `metadata`/`snapshot`/`event` frames, the real-session diagnostic script, stop, and proxied live endpoints:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1
```

For a quicker local loop, skip only the live smoke process gate:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -SkipLiveSmoke
```

Real OpenF1 live validation still needs an active race or sprint window. During a real session:

1. Start the backend with OpenF1 live enabled, and set any required OpenF1 live auth variables if your deployment needs them.
2. Before the session opens, run `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -ExpectedNextMeetingName "British Grand Prix" -ExpectedNextSessionType sprint` to confirm OpenF1 is advertising the expected next live session.
3. Run `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -WaitForActiveSeconds 28800 -Start -RequireActive -Strict` before the session window, or `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -Start -RequireActive -Strict` once the session is already active. Confirm it reports `availability: active`, `active: true`, and the expected race or sprint session. Add `-ExpectedMeetingName "British Grand Prix" -ExpectedSessionType sprint` or `-ExpectedSessionType race` to fail fast on the wrong session. If you know the OpenF1 session key, add `-ExpectedSessionKey 12345` too.
4. Open the app or press `OPEN LIVE`; the dashboard should switch to `LIVE · OpenF1` and connect without showing a stale historical replay.
5. Verify the first snapshot reflects the current race state, not session start, with timing, map, weather, race-control, and stint panels all updating from one snapshot.
6. Run `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -WatchSeconds 900 -Strict` to watch live status for 15 minutes and fail immediately on failed, stale, or missing critical channels, a stuck backend runtime, a live snapshot clock that is more than 30 seconds behind/ahead of the OpenF1 session clock, or an empty timing/map snapshot. Strict mode allows degradable channels such as intervals, pit, race-control, stints, weather, and session-result to be bad without failing when the core dashboard snapshot is healthy; override that with `-AllowedBadChannels @()` when investigating those feeds. Add `-MinGeometryPoints 20` once cached, historical, or accumulated live location geometry is expected. Add `-MinEvents 1` once race-control, pit, weather, or derived events are expected. Omit `-Strict` only when intentionally observing degraded upstream behavior.
7. Reload the browser mid-session and confirm it reconnects near the current race state. If OpenF1 discovery is degraded but the backend live session is already running, `/api/live/current` should still report the active in-memory session. Then stop live mode and confirm the app returns cleanly to replay selection/playback.

For terminal help on the live checker, run `Get-Help .\scripts\check-live-current.ps1 -Detailed`.

To validate a second cached historical race, pass its OpenF1 `SessionKey`, `MeetingKey`, expected race name, and quality expectations, for example:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke-mvp.ps1 -SessionKey 9999 -MeetingKey 2222 -ExpectedMeetingName "Japanese Grand Prix" -ExpectedGeometrySource fast_f1_telemetry -ExpectedMapMode gps -ExpectedTrackQuality interpolated
```

Smoke process logs are written under `tmp/`. If the backend or Vite exits before becoming healthy, the smoke script reports the tail of the relevant error log instead of only timing out.

The equivalent manual commands are:

```bash
cargo test
cd frontend
bun run test
bun run build
```

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke-mvp.ps1
```

Current coverage includes replay determinism, OpenF1 normalization, cached rebuilds, Bahrain curated/projected map fallback, OpenF1 live discovery/start/snapshot/status/events/geometry/stream/stop behavior against mocks, mid-session live sync, typed live SSE event frames, REST route contracts, replay query validation, replay event contracts, SSE route contracts and smoke coverage, static PowerShell script parsing, frontend formatters, playback helpers, session selection/readiness helpers, live source/channel badges, timing display and empty-state helpers, event-feed helpers, weather display helpers, stint timeline helpers, track geometry helpers, and track map view helpers.
