# Interval

Replay-first F1 second-screen dashboard for historical race sessions.

The MVP target is historical race replay using FastF1 data cached in SQLite, backend-owned replay snapshots, and a dense engineer-inspired SolidJS dashboard. The 2024 Bahrain Grand Prix race (`session_key=9472`) remains the seeded example and resolver override.

## Current Shape

- `backend/`: Rust API service with Axum, SQLite, FastF1 historical ingest, OpenF1 discovery, replay generation, track geometry, and SSE streaming.
- `frontend/`: SolidJS, TypeScript, Vite, and Tailwind dashboard.
- `shared/`: public replay API contract docs and TypeScript wire types.
- `infra/`: early deployment notes.
- `docs/`: local reference notes and inspiration material; this directory is ignored by Git in this workspace.

The reference `docs/f1-race-replay` project is used for modeling ideas only. This project does not port its Python/UI stack.

## MVP Replay Path

The frontend opens the last selected cached session when available. If that replay is missing or stale, the app clears the active replay and leaves the selector in control instead of falling back to Bahrain. If a selected historical replay is not cached yet, the dashboard prompts the user to choose `INGEST + OPEN`. The seeded Bahrain race (`session_key=9472`) remains available as a known-good FastF1 override, and the seeded demo replay (`session_key=9839`) remains available for offline UI development.

For historical replays, FastF1 telemetry is the preferred source for driver locations and track geometry. Race ingest resolves OpenF1 meeting/session metadata to a FastF1 year, round, and race session code, with curated overrides for known special cases. For older cached OpenF1 data, the backend still:

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

If Bahrain has not been ingested in the local database, use the session selector in the app and choose `INGEST + OPEN`.

## Runtime Configuration

Backend environment variables:

- `DATABASE_URL`: SQLite URL, defaults to `sqlite://interval.db`.
- `INTERVAL_BIND`: backend bind address, defaults to `127.0.0.1:4000`.
- `INTERVAL_REBUILD_SESSION_ON_START`: optional session key to rebuild replay artifacts from cached raw historical data during backend startup. The smoke script uses `9472`.
- `INTERVAL_FASTF1_PYTHON`: optional Python executable with FastF1 dependencies already installed. If omitted, the backend creates `cache/fastf1-venv` and installs `scripts/fastf1-requirements.txt` on first FastF1 ingest.
- `INTERVAL_FASTF1_BOOTSTRAP_PYTHON`: optional Python executable used to create the managed FastF1 venv. Defaults to `python`.
- `RUST_LOG`: tracing filter, defaults to `interval_backend=info,tower_http=info`.

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

The static check parses the PowerShell scripts. The main verification script runs that static check, backend tests, frontend tests, and the frontend production build.
The script checks for required local tools (`cargo` and `bun`) before running the gates.

After a historical race has been ingested once into `interval.db`, run the full cached MVP verification. With no extra parameters this checks the seeded Bahrain target:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -WithSmoke
```

The smoke step builds a fresh backend binary, rebuilds the requested replay artifacts from the cached raw historical bundle, starts the backend and Vite dev server, confirms the selected replay is cached, then checks replay metadata, meeting context, a mid-race snapshot with section quality labels and 3-lap derived metrics, invalid replay-cursor handling, track geometry, REST/proxied replay events, initial SSE stream events, metadata-driven stream cadence, and the Vite API proxy before shutting the local smoke processes down.

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

Current coverage includes replay determinism, OpenF1 normalization, cached rebuilds, Bahrain curated/projected map fallback, REST route contracts, replay query validation, replay event contracts, SSE route contracts and smoke coverage, static PowerShell script parsing, frontend formatters, playback helpers, session selection/readiness helpers, timing display and empty-state helpers, event-feed helpers, weather display helpers, stint timeline helpers, track geometry helpers, and track map view helpers.
