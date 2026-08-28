//! Runs the backend in-process: the replacement for the electron shell's
//! `desktop/src/backend.js` child-process supervision.
//!
//! The backend resolves every path it touches (.env, interval.db, backend/assets/tracks,
//! scripts/, cache/) relative to its working directory, so the single most important
//! thing here is entering a writable per-user data directory before touching it.
//! The `backend/assets/tracks` nesting is not a style choice: curated_tracks.rs
//! hardcodes ASSETS_DIR = "backend/assets/tracks" relative to cwd. Get this wrong and
//! the backend silently falls back to a stub track outline instead of erroring.

use include_dir::{Dir, include_dir};
use interval_backend::server::{self, BoundServer, ServeOptions};
use std::future::Future;
use std::path::{Path, PathBuf};

static TRACK_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../backend/assets/tracks");
static FASTF1_SCRIPT: &str = include_str!("../../scripts/fastf1_export_replay.py");
static FASTF1_REQUIREMENTS: &str = include_str!("../../scripts/fastf1-requirements.txt");
static ENV_EXAMPLE: &str = include_str!("../../.env.example");
// A pinned uv binary, downloaded and sha256-verified by build.rs. Empty when embedding
// was skipped (unsupported target or INTERVAL_SKIP_UV_EMBED).
static UV_BINARY: &[u8] = include_bytes!(env!("INTERVAL_UV_EMBED_PATH"));
const UV_VERSION: &str = env!("INTERVAL_UV_EMBED_VERSION");

/// The per-user data directory the embedded backend runs in.
///
/// A `data` subdirectory of the platform app-data dir, mirroring the electron shell's
/// `<userData>/data` layout — on macOS and Windows (case-insensitive filesystems) this
/// is the *same* directory the electron build uses, so an existing install keeps its
/// ingested sessions; on Linux it is `~/.local/share/interval/data` where electron used
/// `~/.config/interval/data`. `INTERVAL_DESKTOP_DATA_DIR` overrides it for development.
/// Must be called before [`scrub_env`], which removes the override variable.
pub fn data_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("INTERVAL_DESKTOP_DATA_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(|home| {
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
            })
    } else {
        // The XDG spec says a relative $XDG_DATA_HOME must be ignored.
        std::env::var_os("XDG_DATA_HOME")
            .filter(|v| v.to_string_lossy().starts_with('/'))
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|home| PathBuf::from(home).join(".local").join("share"))
            })
    };
    let base = base.ok_or_else(|| anyhow::anyhow!("no home directory; cannot pick a data dir"))?;
    Ok(base.join("interval").join("data"))
}

/// Mirror the read-only payload embedded in the binary into the writable data dir.
///
/// The payload is small and is always exactly what shipped, so it is overwritten
/// unconditionally rather than version-stamped — an app update propagates on next
/// launch, matching backend.js semantics.
pub fn prepare_data_dir(dir: &PathBuf) -> anyhow::Result<()> {
    for sub in ["logs", "scripts", "backend/assets/tracks", "cache"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }

    for file in TRACK_ASSETS.files() {
        std::fs::write(
            dir.join("backend/assets/tracks").join(file.path()),
            file.contents(),
        )?;
    }

    // If the FastF1 requirements changed since last launch, drop only the readiness
    // sentinel. ensure_managed_python then re-runs `pip install -r` against the existing
    // venv instead of rebuilding it — without this, an app update never upgrades FastF1.
    let req_dst = dir.join("scripts/fastf1-requirements.txt");
    let req_changed = match std::fs::read_to_string(&req_dst) {
        Ok(existing) => existing != FASTF1_REQUIREMENTS,
        Err(_) => true,
    };
    std::fs::write(dir.join("scripts/fastf1_export_replay.py"), FASTF1_SCRIPT)?;
    std::fs::write(&req_dst, FASTF1_REQUIREMENTS)?;
    if req_changed {
        let _ = std::fs::remove_file(dir.join("cache/fastf1-venv/.interval-fastf1-ready"));
    }

    // Seed .env once so the remaining INTERVAL_* switches have an obvious home. The
    // OpenF1 token belongs in the settings panel now. Never overwrite: it may hold
    // credentials.
    let env_dst = dir.join(".env");
    if !env_dst.exists() {
        std::fs::write(&env_dst, ENV_EXAMPLE)?;
    }
    Ok(())
}

/// The backend reads .env only for keys absent from its process environment, so anything
/// inherited from the launching shell silently overrides the user's own `<data>/.env`.
/// Worse, a leaked INTERVAL_REBUILD_SESSION_ON_START aborts startup before the window
/// ever appears. So: strip everything the app owns before the backend reads any of it.
/// RUST_LOG is kept — it only steers this process's own logging.
///
/// Must run before the tokio runtime exists: mutating the environment is only sound
/// while the process is single-threaded, which is why this is `unsafe` in edition 2024.
pub fn scrub_env() {
    let owned: Vec<String> = std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| key.starts_with("INTERVAL_") || key == "DATABASE_URL")
        .collect();
    for key in owned {
        unsafe { std::env::remove_var(&key) };
    }
}

/// Install the embedded uv binary into `<data>/bin` and point the backend's FastF1
/// connector at it via `INTERVAL_FASTF1_UV`, so historical ingest provisions its own
/// Python instead of requiring one on the machine. The file is version-stamped and
/// written once; stale versions from earlier app builds are swept. Any failure only
/// degrades to the system-python bootstrap, so it warns rather than aborts.
///
/// Must run after [`scrub_env`] (which strips `INTERVAL_*`) and, like it, before the
/// tokio runtime exists — `set_var` needs a single-threaded process.
pub fn provision_uv(dir: &Path) {
    if UV_BINARY.is_empty() {
        return;
    }
    let bin_dir = dir.join("bin");
    let name = if cfg!(windows) {
        format!("uv-{UV_VERSION}.exe")
    } else {
        format!("uv-{UV_VERSION}")
    };
    let path = bin_dir.join(&name);
    if !path.exists() {
        if let Err(error) = install_uv_binary(&bin_dir, &name, &path) {
            eprintln!(
                "failed to install embedded uv ({error}); FastF1 ingest will fall back to a system Python"
            );
            return;
        }
    }
    unsafe { std::env::set_var("INTERVAL_FASTF1_UV", &path) };
}

fn install_uv_binary(bin_dir: &Path, name: &str, path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(bin_dir)?;
    if let Ok(entries) = std::fs::read_dir(bin_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            if file_name.to_string_lossy().starts_with("uv-") && file_name != *name {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    // Stage-and-rename so a crash mid-write can't leave a half binary that the
    // `path.exists()` fast path would then trust forever.
    let staged = bin_dir.join(format!("{name}.partial"));
    std::fs::write(&staged, UV_BINARY)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, path)?;
    Ok(())
}

/// Start the embedded server on an ephemeral loopback port. An ephemeral port kills
/// clashes with a dev backend on 4000; the UI learns the base URL straight from the
/// returned address. The settings API is loopback-only by construction here — it must
/// never be enabled on anything reachable beyond 127.0.0.1.
pub async fn serve_embedded(
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<BoundServer> {
    server::serve(
        ServeOptions {
            bind: "127.0.0.1:0".parse().expect("loopback addr parses"),
            database_url: "sqlite://interval.db".to_string(),
            enable_settings_api: true,
        },
        shutdown,
    )
    .await
}
