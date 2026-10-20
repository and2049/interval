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

### Historical data (F1 TV)

Downloading a historical session needs a Formula 1 account with an active F1 TV subscription: the F1 archive now requires it, and the bundled FastF1 is pinned to the `3.9.0a0` prerelease because that is the first release that can sign in. Open the gear menu and fill in the **F1 TV account** section with the email and password of your Formula 1 account. If you subscribe through Apple TV, sign in with the Formula 1 account the subscription is linked to, not your Apple ID; link it first at [account.formula1.com](https://account.formula1.com/#/en/partner-account-sync?country=us&lead_source=web_syndication_apple_us&partnerId=syn_apple_us) if you have not already.

SIGN IN saves the login and runs a real sign-in; the section shows when the login was last verified, and only says "Signed in" after that check has passed. FastF1 performs the sign-in at the start of every download and keeps the resulting token in memory for that run only, so nothing is written to disk, a changed password or account is always used on the next download, and your own FastF1 installs' token file is never touched. Sessions downloaded earlier keep working without a login.

Headless or `cargo run` backends can set `F1_USERNAME` and `F1_PASSWORD` instead; a login saved in the app takes precedence. The exporter never prompts for credentials, so a missing or rejected login fails the download with a clear error instead of hanging.

### Live data

Open the gear menu and sign in with an [OpenF1 account with live access](https://openf1.org/auth.html) in the **OpenF1 account** section. Access tokens refresh automatically.

### Credential storage

Both logins are stored in plain text in `interval/settings.json` under your platform's configuration directory: `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, or `${XDG_CONFIG_HOME:-~/.config}` on Linux. Each login is saved and cleared independently.

### Data and settings

Downloaded sessions, caches, logs, and a user-editable `.env` file live here:

| Platform | Data directory |
| --- | --- |
| Windows | `%APPDATA%\interval\data` |
| macOS | `~/Library/Application Support/interval/data` |
| Linux | `~/.local/share/interval/data` |

While the app runs, `server.json` in this directory records the embedded backend's loopback address (`url`), process id and start time, so local companion tools can find it. It is removed on exit; a stale copy after a crash fails `GET <url>/healthz`. The backend only ever listens on `127.0.0.1`.

Set `INTERVAL_DESKTOP_DATA_DIR` in your environment to use another directory. For other settings, edit `.env` there and restart the app. Environment variables take precedence over `.env`.

| Variable | Purpose |
| --- | --- |
| `INTERVAL_OPENF1_LIVE_ENABLED` | Set to `false` to disable live discovery and polling. Default: `true`. |
| `INTERVAL_OPENF1_LIVE_TOKEN` | Optional bearer token when no login is saved. Saved credentials take precedence. |
| `F1_USERNAME` / `F1_PASSWORD` | Formula 1 account for historical downloads when no login is saved in the app. Saved credentials take precedence. |
| `INTERVAL_OPENF1_LIVE_BASE_URL` | Custom live API endpoint. Default: `https://api.openf1.org/v1/`. |
| `INTERVAL_OPENF1_LIVE_AUTH_HEADER` | Custom token header. Default: `Authorization`. |
| `RUST_LOG` | Diagnostic logging filter, such as `interval_backend=debug`. |
