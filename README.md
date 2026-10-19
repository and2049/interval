# Interval

A desktop F1 second-screen dashboard for live races and historical replays, with timing, track positions, weather, race control, and tyre stints. Supports race and sprint sessions using OpenF1 and FastF1.

## Install

Available for **Windows x64**, **macOS (Apple silicon)**, and **Linux x64**.

**macOS / Linux**

```bash
curl -fsSL https://github.com/and2049/interval/releases/latest/download/install | bash
```

**Windows (PowerShell)**

```powershell
irm https://github.com/and2049/interval/releases/latest/download/install.ps1 | iex
```

The installer adds `interval-desktop` to your `PATH`. Run it to launch the app; Linux also gets an app launcher. You can also download binaries from [Releases](https://github.com/and2049/interval/releases).

Select a session to download and replay it. Python and FastF1 are set up automatically on first use. Updates are available through the gear menu, or by re-running the installer.

## Runtime configuration

### Live data

Open the gear menu and sign in with an [OpenF1 account with live access](https://openf1.org/auth.html). Historical replays do not require a login. Access tokens refresh automatically.

Credentials are stored in plain text in `interval/settings.json` under your platform's configuration directory: `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, or `${XDG_CONFIG_HOME:-~/.config}` on Linux.

### Data and settings

Downloaded sessions, caches, logs, and a user-editable `.env` file live here:

| Platform | Data directory |
| --- | --- |
| Windows | `%APPDATA%\interval\data` |
| macOS | `~/Library/Application Support/interval/data` |
| Linux | `~/.local/share/interval/data` |

Set `INTERVAL_DESKTOP_DATA_DIR` in your environment to use another directory. For other settings, edit `.env` there and restart the app. Environment variables take precedence over `.env`.

| Variable | Purpose |
| --- | --- |
| `INTERVAL_OPENF1_LIVE_ENABLED` | Set to `false` to disable live discovery and polling. Default: `true`. |
| `INTERVAL_OPENF1_LIVE_TOKEN` | Optional bearer token when no login is saved. Saved credentials take precedence. |
| `INTERVAL_OPENF1_LIVE_BASE_URL` | Custom live API endpoint. Default: `https://api.openf1.org/v1/`. |
| `INTERVAL_OPENF1_LIVE_AUTH_HEADER` | Custom token header. Default: `Authorization`. |
| `RUST_LOG` | Diagnostic logging filter, such as `interval_backend=debug`. |
