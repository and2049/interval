# Interval

Replay-first F1 second-screen dashboard for historical race sessions.

The MVP target is the 2024 Bahrain Grand Prix race (`session_key=9472`) using cached OpenF1 historical data, backend-owned replay snapshots, and a dense engineer-inspired SolidJS dashboard.

## Current Shape

- `backend/`: Rust API service with Axum, SQLite, OpenF1 ingestion, replay generation, track geometry, and SSE streaming.
- `frontend/`: SolidJS, TypeScript, Vite, and Tailwind dashboard.
- `shared/`: public replay API contract docs and TypeScript wire types.
- `infra/`: early deployment notes.
- `docs/`: local reference notes and inspiration material; this directory is ignored by Git in this workspace.

The reference `docs/f1-race-replay` project is used for modeling ideas only. This project does not port its Python/UI stack.

## MVP Replay Path

The frontend defaults toward `session_key=9472`. If the historical replay is not cached yet, the dashboard stays on Bahrain and prompts the user to choose `INGEST + OPEN`. The seeded demo replay (`session_key=9839`) remains available from the session selector for offline UI development.

For Bahrain, OpenF1 `location` data can be sparse or missing. The backend therefore:

- prefers usable OpenF1 location geometry when present;
- falls back to curated Bahrain geometry for `session_key=9472`;
- projects driver dots onto that centerline for the map;
- keeps timing order based on OpenF1 position/interval data, not map projection.

Replay payloads use `contract_version: "replay.v1"`. REST snapshots and SSE snapshots share the same nested shape. The frontend advances a local smooth cursor for playback controls, but snapshot requests are quantized to the backend `frame_step_seconds` so the dashboard does not refetch the same persisted frame on every animation tick.

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
- `INTERVAL_REBUILD_SESSION_ON_START`: optional session key to rebuild replay artifacts from cached raw OpenF1 data during backend startup. The smoke script uses `9472`.
- `RUST_LOG`: tracing filter, defaults to `interval_backend=info,tower_http=info`.

Smoke script parameters:

- `-DatabaseUrl`: SQLite URL passed to the backend process.
- `-BackendBind`: backend bind address used by the smoke backend.
- `-FrontendPort`: Vite port used by the smoke frontend.
- `-TimeoutSeconds`: health-check and stream-read timeout.

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

After Bahrain has been ingested once into `interval.db`, run the full cached MVP verification:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -WithSmoke
```

The smoke step builds a fresh backend binary, rebuilds Bahrain replay artifacts from the cached raw OpenF1 bundle, starts the backend and Vite dev server, first confirms Bahrain is cached, then checks replay metadata, meeting context, a mid-race snapshot with section quality labels and 3-lap derived metrics, invalid replay-cursor handling, curated track geometry, REST/proxied replay events, initial SSE stream events, and the Vite API proxy before shutting the local smoke processes down.

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
