# Interval

Live and replay F1 second-screen dashboard for race and sprint sessions.

The app uses one backend-owned `replay.v1` snapshot contract for OpenF1-backed live races, deterministic live simulation, and FastF1 historical replay cached in SQLite. The 2024 Bahrain Grand Prix race (`session_key=9472`) remains the seeded example and resolver override.

## Current Shape

- `backend/`: Rust API service with Axum, SQLite, FastF1 historical ingest, OpenF1 discovery/live polling, replay generation, track geometry, and SSE streaming.
- `frontend/`: SolidJS, TypeScript, Vite, and Tailwind dashboard.
- `desktop/`: Electron shell that runs the backend as a child process and loads the built dashboard from it.
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

Desktop shell (optional, two terminals total):

```bash
# terminal 1
cargo run -p interval-backend

# terminal 2
cd desktop
bun install
bun run dev
```

`bun run dev` in `desktop/` starts Vite and Electron together and stops both when either exits. It deliberately does not start the backend, so cargo rebuilds and backend logs stay in their own terminal. The window loads the Vite dev server, so frontend hot reload works exactly as it does in a browser. Launch order does not matter; the window retries until a server answers.

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
- `INTERVAL_FASTF1_UV`: optional path to a [uv](https://github.com/astral-sh/uv) binary used to bootstrap the FastF1 venv instead of a system Python — uv downloads a managed CPython, so no Python needs to be installed at all. The GPUI desktop app embeds uv and sets this automatically; everything uv touches stays under `cache/`. Ignored when `INTERVAL_FASTF1_PYTHON` is set.
- `INTERVAL_FASTF1_BOOTSTRAP_PYTHON`: optional Python executable used to create the managed FastF1 venv when `INTERVAL_FASTF1_UV` is not set. Defaults to `python` on Windows and `python3` elsewhere.
- `INTERVAL_STATIC_DIR`: optional directory containing a built frontend (`index.html` plus `assets/`). When set, the backend serves it from the same origin as the API, so the dashboard and its SSE streams need no separate web server. Declared routes always win over static files, and startup fails immediately if the directory has no `index.html`. Unset for the web deployment and for local dev, where Vite serves the UI; the desktop shell sets it.
- `INTERVAL_SHUTDOWN_ON_STDIN_EOF`: when set, the backend also shuts down gracefully once its standard input reaches end-of-file. This lets a supervising process stop it by closing the pipe, and guarantees it exits if that parent dies. Unset for normal terminal and deployment runs, where Ctrl-C is the only shutdown trigger; the desktop shell sets it.
- `INTERVAL_OPENF1_LIVE_ENABLED`: OpenF1 live discovery and polling are enabled by default; set to `false`, `0`, `no`, or `off` to disable them for offline/dev runs.
- `INTERVAL_OPENF1_LIVE_BASE_URL`: optional OpenF1-compatible live API base URL, defaults to `https://api.openf1.org/v1/`.
- `INTERVAL_OPENF1_LIVE_TOKEN`: optional server-side live API token. With the default `Authorization` header, either `abc123` or `Bearer abc123` is accepted. A token saved through the settings panel takes precedence over this variable.
- `INTERVAL_ENABLE_SETTINGS_API`: when set, exposes the settings routes under `/api/settings/`. These read and write the OpenF1 token, are exempt from the permissive CORS layer, and have no authentication, so they must stay unset on anything reachable beyond `127.0.0.1`. The desktop shell sets it; `cargo run` does not.
- `INTERVAL_OPENF1_LIVE_AUTH_HEADER`: optional auth header name for the token, defaults to `Authorization`.

For local live testing, copy `.env.example` to `.env`. The backend loads `.env` on startup, and shell-provided environment variables still override it.

### Settings file

The OpenF1 token is normally entered in the app's settings panel rather than an env var. It is stored outside both the repository and the app data directory, so a `cargo run` backend and an installed desktop app on the same machine share one token:

| Platform | Path |
| --- | --- |
| Windows | `%APPDATA%\interval\settings.json` |
| macOS | `~/Library/Application Support/interval/settings.json` |
| Linux | `$XDG_CONFIG_HOME/interval/settings.json`, else `~/.config/interval/settings.json` |

The file holds `{ "openf1_token": "..." }` and is written atomically, owner-only (`0600`) on Unix. A missing or corrupt file is ignored rather than fatal. Precedence is: a token saved here wins, otherwise `INTERVAL_OPENF1_LIVE_TOKEN` from the environment or `.env`, otherwise no token. Clearing the token in the panel falls back to the environment value. Saving through the panel applies immediately without a restart; hand-editing the file applies on next start.
- `RUST_LOG`: tracing filter, defaults to `interval_backend=info,tower_http=info`.

With OpenF1 live enabled, the frontend checks `GET /api/live/current` on startup, polls quietly while no live session is active, and keeps warmup checks responsive while OpenF1 rows are starting to publish. The selector bar shows live availability as `LIVE CHECKING`, `LIVE WAITING`, `LIVE READY`, `LIVE OPEN`, `LIVE OFF`, or `LIVE ERROR`, and the control bar exposes `OPEN LIVE` for a manual check/start. A race or sprint becomes active at its OpenF1 session start time; before that it is shown as the next live session so the app does not open live before timing/location rows exist. Once active, the backend starts an in-memory live session, polls OpenF1 channels on endpoint-specific cadences, and streams the same `ReplaySnapshot` shape used by historical replay. Reloading the app during a running backend live session reconnects through that in-memory session even if upstream discovery has a transient failure. Individual OpenF1 live HTTP requests are bounded by a request timeout so one slow endpoint cannot freeze live refresh indefinitely. `LIVE SIM` remains available as a deterministic test mode from cached historical sessions.

Smoke script parameters:

- `-DatabaseUrl`: SQLite URL passed to the backend process.
- `-BackendBind`: backend bind address used by the smoke backend.
- `-FrontendPort`: Vite port used by the smoke frontend.
- `-TimeoutSeconds`: health-check and stream-read timeout.
- `-SessionKey`, `-MeetingKey`, `-ExpectedMeetingName`, and `-SnapshotT`: cached replay target. Defaults cover Bahrain `9472`.
- `-ExpectedMapMode`, `-ExpectedTrackQuality`, `-ExpectedGeometrySource`, and `-MinimumTimingRows`: target-specific replay quality assertions. Defaults match the Bahrain curated/projected fallback.

## Desktop App

The desktop build is an Electron shell around the same backend and the same built frontend. The backend serves the dashboard over `INTERVAL_STATIC_DIR`, so the renderer stays same-origin and no frontend code differs between web and desktop.

Build an installer for the current platform:

```bash
cd desktop
bun install
bun run dist
```

That runs, in order: `cargo build --release -p interval-backend`, the frontend build (which also type-checks), a step that stages the backend binary, then `electron-builder`. Artifacts land in `desktop/release/` — NSIS `.exe` on Windows, `.dmg` on macOS, `.AppImage` on Linux. Each platform must be built on itself; the Rust backend cannot be cross-compiled between them. `.github/workflows/desktop.yml` builds all three on GitHub Actions runners.

At runtime the shell picks a free port starting at `45900`, spawns the backend with a per-user data directory as its working directory, waits for `/healthz`, then loads the dashboard. That data directory is `%APPDATA%\Interval\data` on Windows, `~/Library/Application Support/Interval/data` on macOS, and `~/.config/Interval/data` on Linux. It is a subdirectory rather than the profile root because Electron keeps its own `Cache/` and storage folders there. It holds:

```text
.env                       user-editable configuration
interval.db                the app's database
backend/assets/tracks/     curated track geometry, refreshed each launch
scripts/                   FastF1 export script, refreshed each launch
cache/                     FastF1 venv, HTTP cache, and exported bundles
logs/backend.log           backend output for the current session
```

Notes on the packaged app:

- It starts with an **empty database** and populates it by ingesting sessions on demand. The working `interval.db` in this repository is far too large to ship.
- To use a live OpenF1 token, open the gear menu in the top bar and paste it there. It is written to the shared settings file described above, not to the data directory, so a `cargo run` backend on the same machine picks up the same token. The `.env` in the data directory still works for the other `INTERVAL_*` switches and is overridden by a saved token.
- Historical ingest requires **Python on `PATH`**; the backend builds its own FastF1 virtual environment inside `cache/` on first use. Live timing and already-ingested replays work without Python.
- Installers are unsigned. Windows shows a SmartScreen prompt on first run, and macOS reports the app as damaged unless it is opened via right-click → Open. Signing and notarization are out of scope.

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
