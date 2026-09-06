# Interval

Live and replay F1 second-screen dashboard for race and sprint sessions.

The app uses one backend-owned `replay.v1` snapshot contract for OpenF1-backed live races, deterministic live simulation, and FastF1 historical replay cached in SQLite. The 2024 Bahrain Grand Prix race (`session_key=9472`) remains the seeded example and resolver override.

## Current Shape

- `backend/`: Rust API service with Axum, SQLite, FastF1 historical ingest, OpenF1 discovery/live polling, replay generation, track geometry, and SSE streaming.
- `desktop-core/`: Rust client library shared by the UI — API client, replay store/runtime, playback, formatters, session selection/readiness, timing/weather/stint helpers, track geometry, and track-map view logic.
- `desktop-gpui/`: native [GPUI](https://github.com/zed-industries/zed) desktop app (`interval-desktop` binary). It embeds the backend in-process and renders the dashboard natively, consuming the same HTTP + SSE contract a browser would.
- `shared/`: public replay API contract docs and TypeScript wire types.
- `infra/`: early deployment notes.
- `docs/`: local reference notes and inspiration material; this directory is ignored by Git in this workspace.

The reference `docs/f1-race-replay` project is used for modeling ideas only. This project does not port its Python/UI stack.

## Historical Replay Path

The app opens the last selected cached session when available. If that replay is missing or stale, it clears the active replay and leaves the selector in control instead of falling back to Bahrain. Selecting a race meeting or session is active intent: cached replays open automatically, and uncached historical race or sprint sessions start FastF1 ingest automatically before opening. The seeded Bahrain race (`session_key=9472`) remains available as a known-good FastF1 override, and the seeded demo replay (`session_key=9839`) remains available for offline UI development.

For historical replays, FastF1 telemetry is the preferred source for driver locations and track geometry. Race and sprint ingest resolves OpenF1 meeting/session metadata to a FastF1 year, round, and session code, with curated overrides for known special cases. For older cached OpenF1 data, the backend still:

- prefers usable upstream location geometry when present;
- falls back to curated Bahrain geometry for `session_key=9472`;
- projects driver dots onto that centerline for the map;
- keeps timing order based on normalized position/interval data, not map projection.

Replay payloads use `contract_version: "replay.v1"`. REST snapshots and SSE snapshots share the same nested shape. FastF1 historical replays currently persist frames at 5 Hz (`frame_step_seconds = 0.2`); legacy OpenF1/demo replays may use a lower cadence. The UI advances a local smooth cursor for playback controls, but snapshot requests are quantized to the backend `frame_step_seconds` so the dashboard does not refetch the same persisted frame on every animation tick.

Backend-derived snapshot sections currently include 3-lap pace metrics, map quality labels, race-control history, weather context, timing rows, and projected track positions. Panels read those sections directly instead of inferring state from raw OpenF1 records.

## Run Locally

The desktop app is the primary way to run Interval. It embeds the backend in-process, so a single command starts everything:

```bash
cargo run -p interval-desktop
```

The app binds the backend to an ephemeral loopback port, runs out of a per-user data directory (see [Desktop App](#desktop-app)), and opens the dashboard window. Set `INTERVAL_DESKTOP_DATA_DIR` to point it at a scratch directory during development.

To work on the backend on its own — for `curl`, contract checks, or SSE inspection — run it as a standalone service:

```bash
cargo run -p interval-backend
```

The standalone backend listens on `http://127.0.0.1:4000` and uses `sqlite://interval.db` unless `DATABASE_URL` is set. Because the desktop app uses an ephemeral port, a standalone backend on `4000` does not clash with it.

Useful checks against a standalone backend:

```bash
curl http://127.0.0.1:4000/healthz
curl http://127.0.0.1:4000/api/sessions/9472/replay/metadata
curl "http://127.0.0.1:4000/api/sessions/9472/replay/snapshot?t=600"
curl http://127.0.0.1:4000/api/sessions/9472/track/geometry
```

If Bahrain has not been ingested in the local database, select it in the app; ingest starts automatically. The manual selector button remains available for retry/reload.

## Runtime Configuration

Backend environment variables:

- `DATABASE_URL`: SQLite URL, defaults to `sqlite://interval.db`.
- `INTERVAL_BIND`: standalone backend bind address, defaults to `127.0.0.1:4000`. Ignored by the embedded desktop backend, which always binds an ephemeral loopback port.
- `INTERVAL_REBUILD_SESSION_ON_START`: optional session key to rebuild replay artifacts from cached raw historical data during backend startup.
- `INTERVAL_FASTF1_PYTHON`: optional Python executable with FastF1 dependencies already installed. If omitted, the backend creates `cache/fastf1-venv` and installs `scripts/fastf1-requirements.txt` on first FastF1 ingest.
- `INTERVAL_FASTF1_UV`: optional path to a [uv](https://github.com/astral-sh/uv) binary used to bootstrap the FastF1 venv instead of a system Python — uv downloads a managed CPython, so no Python needs to be installed at all. The desktop app embeds uv and sets this automatically; everything uv touches stays under `cache/`. Ignored when `INTERVAL_FASTF1_PYTHON` is set.
- `INTERVAL_FASTF1_BOOTSTRAP_PYTHON`: optional Python executable used to create the managed FastF1 venv when `INTERVAL_FASTF1_UV` is not set. Defaults to `python` on Windows and `python3` elsewhere.
- `INTERVAL_SHUTDOWN_ON_STDIN_EOF`: when set, the backend also shuts down gracefully once its standard input reaches end-of-file, so a supervising process can stop it by closing the pipe. Unset for normal terminal and deployment runs, where Ctrl-C is the only shutdown trigger.
- `INTERVAL_OPENF1_LIVE_ENABLED`: OpenF1 live discovery and polling are enabled by default; set to `false`, `0`, `no`, or `off` to disable them for offline/dev runs.
- `INTERVAL_OPENF1_LIVE_BASE_URL`: optional OpenF1-compatible live API base URL, defaults to `https://api.openf1.org/v1/`.
- `INTERVAL_OPENF1_LIVE_TOKEN`: optional pre-issued bearer token, sent as-is. With the default `Authorization` header, either `abc123` or `Bearer abc123` is accepted. OpenF1's own tokens expire after an hour, so this is an escape hatch for testing and compatible mirrors; a login saved through the settings panel takes precedence over it.
- `INTERVAL_ENABLE_SETTINGS_API`: when set, exposes the settings routes under `/api/settings/`. These read and write the OpenF1 login, are exempt from the permissive CORS layer, and have no authentication, so they must stay unset on anything reachable beyond `127.0.0.1`. The desktop app enables it on its loopback-only embedded server; `cargo run -p interval-backend` does not.
- `INTERVAL_OPENF1_LIVE_AUTH_HEADER`: optional auth header name for the token, defaults to `Authorization`.

For local live testing, copy `.env.example` to `.env`. The backend loads `.env` on startup, and shell-provided environment variables still override it.

### OpenF1 login

Live data needs an [OpenF1 account](https://openf1.org/auth.html) (historical data is free and unaffected). OpenF1 uses a password grant: the backend posts the account's username and password to `https://api.openf1.org/token`, receives a bearer token that expires after one hour, and sends it as `Authorization: Bearer …` on every live request. The token is fetched on first use, cached, exchanged again two minutes before its stated expiry, and exchanged once more if OpenF1 rejects a cached one mid-session. Nothing needs restarting when it rolls over.

The login is entered in the app's settings panel (the gear in the top bar) rather than an env var. It is stored outside both the repository and the app data directory, so a `cargo run` backend and an installed desktop app on the same machine share one login:

| Platform | Path |
| --- | --- |
| Windows | `%APPDATA%\interval\settings.json` |
| macOS | `~/Library/Application Support/interval/settings.json` |
| Linux | `$XDG_CONFIG_HOME/interval/settings.json`, else `~/.config/interval/settings.json` |

The file holds `{ "openf1_username": "...", "openf1_password": "..." }` in plain text and is written atomically, owner-only (`0600`) on Unix. A missing or corrupt file is ignored rather than fatal, and a leftover `openf1_token` key from older builds is ignored. Precedence is: a login saved here wins, otherwise `INTERVAL_OPENF1_LIVE_TOKEN` from the environment or `.env`, otherwise unauthenticated. Signing out in the panel falls back to the environment value. Saving through the panel applies immediately without a restart and runs a connection test, which exercises the token exchange so a wrong password shows up as `REJECTED` right there; hand-editing the file applies on next start.

- `RUST_LOG`: tracing filter, defaults to `interval_backend=info,tower_http=info`.

With OpenF1 live enabled, the app checks `GET /api/live/current` on startup, polls quietly while no live session is active, and keeps warmup checks responsive while OpenF1 rows are starting to publish. The selector bar shows live availability as `LIVE CHECKING`, `LIVE WAITING`, `LIVE READY`, `LIVE OPEN`, `LIVE OFF`, or `LIVE ERROR`, and the control bar exposes `OPEN LIVE` for a manual check/start. A race or sprint becomes active at its OpenF1 session start time; before that it is shown as the next live session so the app does not open live before timing/location rows exist. Once active, the backend starts an in-memory live session, polls OpenF1 channels on endpoint-specific cadences, and streams the same `ReplaySnapshot` shape used by historical replay. The cadences are budgeted against OpenF1's sponsor-tier limit of 6 requests per second **and 60 per minute**: intervals and location every 5 s, position every 6 s, race control every 8 s, laps every 12 s, slower feeds less often, about 54 requests per minute in total, with a shared limiter as the hard cap. A 429 pauses every poller for the `Retry-After` period. Because location arrives in 5 s batches and OpenF1 itself runs a few seconds behind the track, the live cursor is held 13 s behind wall clock (location cadence plus an 8 s allowance) so it always has real samples on both sides to interpolate between; a shorter delay makes the cars freeze and jump at the end of each poll cycle. OpenF1 publishes no retirement flag until the session ends, so a live retirement is inferred: a car whose location has not moved for 90 s (samples stopped, or a fixed point in the garage) is marked out, its dot leaves the map and its timing row greys out with `OUT`, provided at least half the field moved in that window so a parked grid or a red flag never flags everyone. OpenF1 answers an empty result set with `404 {"detail":"No results found."}`, which is treated as no new rows rather than a failure. Reconnecting during a running backend live session rejoins that in-memory session even if upstream discovery has a transient failure. Individual OpenF1 live HTTP requests are bounded by a request timeout so one slow endpoint cannot freeze live refresh indefinitely. `LIVE SIM` remains available as a deterministic test mode from cached historical sessions.

## Desktop App

The desktop app is a native GPUI window (`interval-desktop`) that embeds the backend in-process. There is no separate web server, browser, or Node runtime: the backend binds `127.0.0.1:0`, and the UI consumes it over HTTP + SSE, so the backend's HTTP contract stays the single source of truth between desktop and any future web client.

Build a release binary for the current platform:

```bash
cargo build --release -p interval-desktop
```

The binary is `target/release/interval-desktop` (`.exe` on Windows). Each platform must be built on itself; the Rust backend and GPUI backends cannot be cross-compiled between them. `.github/workflows/desktop-gpui.yml` builds all three on GitHub Actions runners and uploads the raw binaries as artifacts. (Installer/packaging metadata is a later task; the app currently ships as a bare binary.)

At runtime the app enters a per-user data directory before starting the backend, which resolves every path it touches (`.env`, `interval.db`, `backend/assets/tracks`, `scripts/`, `cache/`) relative to that directory. It is `<app-data>/interval/data`:

| Platform | Data directory |
| --- | --- |
| Windows | `%APPDATA%\interval\data` |
| macOS | `~/Library/Application Support/interval/data` |
| Linux | `~/.local/share/interval/data` |

`INTERVAL_DESKTOP_DATA_DIR` overrides it for development. The directory holds:

```text
.env                       user-editable configuration (seeded from .env.example once)
interval.db                the app's database
backend/assets/tracks/     curated track geometry, refreshed each launch
scripts/                   FastF1 export script + requirements, refreshed each launch
cache/                     FastF1 venv, HTTP cache, and exported bundles
bin/                       embedded uv binary, version-stamped
logs/                      backend output
```

The read-only payload (track assets, FastF1 script, the uv binary) is embedded in the app binary and mirrored into the data directory on each launch, so an app update propagates on next start. On macOS and Windows the data directory matches the electron-era `<userData>/data` layout, so an existing install keeps its ingested sessions.

Notes on the packaged app:

- It starts with an **empty database** and populates it by ingesting sessions on demand. The working `interval.db` in this repository is far too large to ship.
- To watch live sessions, open the gear menu in the top bar and sign in with your OpenF1 account. The login is written to the shared settings file described above, not to the data directory, so a `cargo run` backend on the same machine picks up the same login. The `.env` in the data directory still works for the other `INTERVAL_*` switches and is overridden by a saved login.
- Historical ingest needs Python, but the app embeds `uv` and provisions its own managed CPython + FastF1 environment under `cache/` on first use, so no system Python is required. Live timing and already-ingested replays work without any Python at all.

## Verification

Run the current gates before treating a change as stable.

Rust tests and a release build of the app (this is what `.github/workflows/desktop-gpui.yml` runs):

```bash
cargo test -p interval-backend -p interval-desktop-core
cargo build --release -p interval-desktop
```

Coverage includes replay determinism, OpenF1 normalization, cached rebuilds, Bahrain curated/projected map fallback, OpenF1 live discovery/start/snapshot/status/events/geometry/stream/stop behavior against mocks, mid-session live sync, typed live SSE event frames, REST route contracts, replay query validation, replay event contracts, and SSE route contracts (backend), plus the ported client logic — formatters, playback helpers, session selection/readiness helpers, live source/channel badges, timing display and empty-state helpers, event-feed helpers, weather display helpers, stint timeline helpers, track geometry helpers, track map view helpers, and the store-integration test that drives the embedded backend end-to-end (`desktop-core`).

The remaining PowerShell helpers cover live OpenF1 validation, which cannot run in CI:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-static.ps1
```

`check-static.ps1` parses the PowerShell scripts and checks the live checker's help. Real OpenF1 live validation still needs an active race or sprint window. During a real session:

1. Start the desktop app (or a standalone backend with OpenF1 live enabled), and set any required OpenF1 live auth variables if your deployment needs them.
2. Before the session opens, run `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -ExpectedNextMeetingName "British Grand Prix" -ExpectedNextSessionType sprint` to confirm OpenF1 is advertising the expected next live session.
3. Run `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -WaitForActiveSeconds 28800 -Start -RequireActive -Strict` before the session window, or `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -Start -RequireActive -Strict` once the session is already active. Confirm it reports `availability: active`, `active: true`, and the expected race or sprint session. Add `-ExpectedMeetingName "British Grand Prix" -ExpectedSessionType sprint` or `-ExpectedSessionType race` to fail fast on the wrong session. If you know the OpenF1 session key, add `-ExpectedSessionKey 12345` too.
4. Open the app or press `OPEN LIVE`; the dashboard should switch to `LIVE · OpenF1` and connect without showing a stale historical replay.
5. Verify the first snapshot reflects the current race state, not session start, with timing, map, weather, race-control, and stint panels all updating from one snapshot.
6. Run `powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -WatchSeconds 900 -Strict` to watch live status for 15 minutes and fail immediately on failed, stale, or missing critical channels, a stuck backend runtime, a live snapshot clock that is more than 30 seconds behind/ahead of the OpenF1 session clock, or an empty timing/map snapshot. Strict mode allows degradable channels such as intervals, pit, race-control, stints, weather, and session-result to be bad without failing when the core dashboard snapshot is healthy; override that with `-AllowedBadChannels @()` when investigating those feeds. Add `-MinGeometryPoints 20` once cached, historical, or accumulated live location geometry is expected. Add `-MinEvents 1` once race-control, pit, weather, or derived events are expected. Omit `-Strict` only when intentionally observing degraded upstream behavior.
7. Reconnect mid-session and confirm the app rejoins near the current race state. If OpenF1 discovery is degraded but the backend live session is already running, `/api/live/current` should still report the active in-memory session. Then stop live mode and confirm the app returns cleanly to replay selection/playback.

For terminal help on the live checker, run `Get-Help .\scripts\check-live-current.ps1 -Detailed`.
