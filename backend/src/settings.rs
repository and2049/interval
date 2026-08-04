//! User settings persisted outside the working directory.
//!
//! Everything else this backend reads is resolved relative to the current working
//! directory (`.env`, `interval.db`, `backend/assets/`, `cache/`). Settings are the
//! deliberate exception: they live in the per-user config directory so a `cargo run`
//! backend and an installed desktop app on the same machine share one file, and so a
//! credential is not stored next to the replay data.

use crate::connectors::openf1_live::{OpenF1Auth, OpenF1Login};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "interval";
const FILE_NAME: &str = "settings.json";

/// Settings as stored on disk.
///
/// Deliberately tolerant: unknown keys are ignored so a file written by a newer build
/// still loads, and absent keys fall back to `None` so an older file still loads.
///
/// The OpenF1 login is stored as the account's username and password rather than a
/// token: OpenF1 issues one-hour tokens, so a saved token would be dead by the next
/// session. The file is owner-only, but this is still a plaintext password on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openf1_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openf1_password: Option<String>,
}

impl Settings {
    /// The saved login, or `None` unless both halves are present and non-blank.
    pub fn openf1_login(&self) -> Option<OpenF1Login> {
        Some(OpenF1Login {
            username: non_blank(self.openf1_username.clone())?,
            password: non_blank(self.openf1_password.clone())?,
        })
    }
}

/// Where the effective OpenF1 credentials came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    Settings,
    Env,
    None,
}

impl AuthSource {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthSource::Settings => "settings",
            AuthSource::Env => "env",
            AuthSource::None => "none",
        }
    }
}

/// A login saved through the settings UI wins over `INTERVAL_OPENF1_LIVE_TOKEN`.
///
/// The environment value is the one the user cannot reach from the UI, so if it won,
/// saving a login would silently do nothing on any machine that has a `.env`.
pub fn resolve_openf1_auth(settings: &Settings, env_token: Option<String>) -> (OpenF1Auth, AuthSource) {
    match settings.openf1_login() {
        Some(login) => (OpenF1Auth::Login(login), AuthSource::Settings),
        None => match non_blank(env_token) {
            Some(token) => (OpenF1Auth::Token(token), AuthSource::Env),
            None => (OpenF1Auth::None, AuthSource::None),
        },
    }
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

/// Resolve the per-user config directory. Split out from the environment so it can be
/// tested for every platform without touching the real one.
fn settings_dir_from(
    os: &str,
    appdata: Option<&OsStr>,
    xdg: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    let base = match os {
        "windows" => PathBuf::from(non_empty(appdata)?),
        "macos" => PathBuf::from(non_empty(home)?)
            .join("Library")
            .join("Application Support"),
        // The XDG spec says a relative $XDG_CONFIG_HOME must be ignored. This branch is
        // POSIX-only, so test for a leading slash rather than `Path::is_absolute`, which
        // answers for the host platform and would make this function impure.
        _ => match non_empty(xdg).filter(|value| value.to_string_lossy().starts_with('/')) {
            Some(value) => PathBuf::from(value),
            None => PathBuf::from(non_empty(home)?).join(".config"),
        },
    };
    Some(base.join(APP_DIR))
}

fn non_empty(value: Option<&OsStr>) -> Option<&OsStr> {
    value.filter(|value| !value.is_empty())
}

/// The per-user config directory (`<config>/interval`), or `None` if the platform's
/// config directory cannot be determined. Shared with the desktop frontend, which keeps
/// its own state files (window bounds, last session) next to `settings.json`.
pub fn config_dir() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let appdata = std::env::var_os("APPDATA");
    let xdg = std::env::var_os("XDG_CONFIG_HOME");
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    settings_dir_from(os, appdata.as_deref(), xdg.as_deref(), home.as_deref())
}

/// `<config dir>/interval/settings.json`, or `None` if the platform's config directory
/// cannot be determined (in which case settings are simply unavailable, not fatal).
pub fn default_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(FILE_NAME))
}

/// Reads settings, treating every failure as "no settings".
///
/// A missing, unreadable, or corrupt file must never prevent the backend from starting,
/// so nothing here propagates. A file that exists but will not parse is worth a warning.
pub fn load_from(path: &Path) -> Settings {
    let Ok(raw) = fs::read_to_string(path) else {
        return Settings::default();
    };
    match serde_json::from_str(&raw) {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "ignoring unreadable settings file"
            );
            Settings::default()
        }
    }
}

/// Writes settings atomically, and readable only by the owner on Unix.
pub fn save_to(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("settings path has no parent directory"))?;
    fs::create_dir_all(parent)?;

    // Write-then-rename so a crash mid-write cannot truncate an existing login, and set
    // the mode on the temporary file so the credential is never briefly world-readable.
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(settings)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

/// Handle to the settings file. Holds the path so tests can point at a temporary file
/// instead of the developer's real config directory.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: Option<PathBuf>,
}

impl SettingsStore {
    pub fn default_location() -> Self {
        Self {
            path: default_path(),
        }
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub fn load(&self) -> Settings {
        self.path
            .as_deref()
            .map(load_from)
            .unwrap_or_else(Settings::default)
    }

    /// Load-modify-write, so keys this build does not set are preserved.
    /// `None` removes the login entirely rather than storing empty strings.
    pub fn store_login(&self, login: Option<&OpenF1Login>) -> anyhow::Result<()> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("no writable settings directory for this platform"))?;
        let mut settings = load_from(path);
        settings.openf1_username = login.map(|login| login.username.clone());
        settings.openf1_password = login.map(|login| login.password.clone());
        save_to(path, &settings)
    }

    pub fn path_display(&self) -> Option<String> {
        self.path
            .as_deref()
            .map(|path| path.display().to_string())
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::default_location()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn os(value: &str) -> OsString {
        OsString::from(value)
    }

    #[test]
    fn windows_uses_appdata() {
        let dir = settings_dir_from(
            "windows",
            Some(&os(r"C:\Users\sun\AppData\Roaming")),
            None,
            None,
        );
        assert_eq!(
            dir,
            Some(PathBuf::from(r"C:\Users\sun\AppData\Roaming").join("interval"))
        );
    }

    #[test]
    fn macos_uses_application_support() {
        let dir = settings_dir_from("macos", None, None, Some(&os("/Users/sun")));
        assert_eq!(
            dir,
            Some(PathBuf::from("/Users/sun/Library/Application Support/interval"))
        );
    }

    #[test]
    fn linux_prefers_absolute_xdg_config_home() {
        let dir = settings_dir_from(
            "linux",
            None,
            Some(&os("/custom/config")),
            Some(&os("/home/sun")),
        );
        assert_eq!(dir, Some(PathBuf::from("/custom/config/interval")));
    }

    #[test]
    fn linux_ignores_relative_xdg_config_home() {
        let dir = settings_dir_from(
            "linux",
            None,
            Some(&os("relative/config")),
            Some(&os("/home/sun")),
        );
        assert_eq!(dir, Some(PathBuf::from("/home/sun/.config/interval")));
    }

    #[test]
    fn linux_falls_back_to_home_config() {
        let dir = settings_dir_from("linux", None, None, Some(&os("/home/sun")));
        assert_eq!(dir, Some(PathBuf::from("/home/sun/.config/interval")));
    }

    #[test]
    fn missing_home_yields_no_settings_dir() {
        assert_eq!(settings_dir_from("linux", None, None, None), None);
        assert_eq!(settings_dir_from("windows", Some(&os("")), None, None), None);
    }

    fn saved(username: &str, password: &str) -> Settings {
        Settings {
            openf1_username: Some(username.to_string()),
            openf1_password: Some(password.to_string()),
        }
    }

    #[test]
    fn resolve_prefers_a_saved_login_over_an_env_token() {
        let (auth, source) =
            resolve_openf1_auth(&saved("me@example.com", "pw"), Some("from-env".to_string()));
        assert_eq!(
            auth,
            OpenF1Auth::Login(OpenF1Login {
                username: "me@example.com".to_string(),
                password: "pw".to_string(),
            })
        );
        assert_eq!(source, AuthSource::Settings);
    }

    #[test]
    fn resolve_falls_back_to_env_token() {
        let (auth, source) = resolve_openf1_auth(&Settings::default(), Some("from-env".to_string()));
        assert_eq!(auth, OpenF1Auth::Token("from-env".to_string()));
        assert_eq!(source, AuthSource::Env);
    }

    #[test]
    fn resolve_treats_a_half_blank_login_as_absent() {
        let (auth, source) =
            resolve_openf1_auth(&saved("me@example.com", "   "), Some("from-env".to_string()));
        assert_eq!(auth, OpenF1Auth::Token("from-env".to_string()));
        assert_eq!(source, AuthSource::Env);

        let (auth, _) = resolve_openf1_auth(&saved("", "pw"), None);
        assert_eq!(auth, OpenF1Auth::None);
    }

    #[test]
    fn resolve_reports_none_when_nothing_is_configured() {
        let (auth, source) = resolve_openf1_auth(&Settings::default(), Some("  ".to_string()));
        assert_eq!(auth, OpenF1Auth::None);
        assert_eq!(source, AuthSource::None);
    }

    #[test]
    fn login_debug_output_redacts_the_password() {
        let login = OpenF1Login {
            username: "me@example.com".to_string(),
            password: "hunter2".to_string(),
        };
        let printed = format!("{login:?}");
        assert!(printed.contains("me@example.com"));
        assert!(!printed.contains("hunter2"), "{printed}");
    }

    #[test]
    fn load_from_tolerates_missing_corrupt_and_future_files() {
        let dir = temp_dir();
        let missing = dir.join("missing.json");
        assert_eq!(load_from(&missing), Settings::default());

        let corrupt = dir.join("corrupt.json");
        fs::write(&corrupt, "not json at all").unwrap();
        assert_eq!(load_from(&corrupt), Settings::default());

        let empty = dir.join("empty.json");
        fs::write(&empty, "{}").unwrap();
        assert_eq!(load_from(&empty).openf1_login(), None);

        // A file written by a newer build with keys this one does not know about, and
        // one from the token era: the stale `openf1_token` is ignored, not an error.
        let future = dir.join("future.json");
        fs::write(
            &future,
            r#"{"openf1_username":"me@example.com","openf1_password":"pw","openf1_token":"old","future_key":42}"#,
        )
        .unwrap();
        assert_eq!(
            load_from(&future).openf1_login().map(|login| login.username),
            Some("me@example.com".to_string())
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_login_round_trips_and_clearing_removes_the_keys() {
        let dir = temp_dir();
        let path = dir.join("settings.json");
        let store = SettingsStore::at(path.clone());
        let login = OpenF1Login {
            username: "me@example.com".to_string(),
            password: "a-password".to_string(),
        };

        store.store_login(Some(&login)).unwrap();
        assert_eq!(store.load().openf1_login(), Some(login));

        store.store_login(None).unwrap();
        assert_eq!(store.load().openf1_login(), None);
        // Assert on the bytes: a cleared login must be absent, not serialized as null.
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("openf1_"), "unexpected file body: {raw}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_login_creates_missing_directories() {
        let dir = temp_dir();
        let path = dir.join("nested").join("deeper").join("settings.json");
        SettingsStore::at(path.clone())
            .store_login(Some(&OpenF1Login {
                username: "me@example.com".to_string(),
                password: "pw".to_string(),
            }))
            .unwrap();
        assert!(path.is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "interval-settings-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
