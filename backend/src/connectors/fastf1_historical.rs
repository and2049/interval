use crate::{connectors::openf1_historical::RawEndpoint, domain::Session};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use thiserror::Error;

const SCRIPT_PATH: &str = "scripts/fastf1_export_replay.py";
const REQUIREMENTS_PATH: &str = "scripts/fastf1-requirements.txt";
const DEFAULT_CACHE_DIR: &str = "cache/fastf1";
const DEFAULT_VENV_DIR: &str = "cache/fastf1-venv";
const READY_SENTINEL: &str = ".interval-fastf1-ready";

#[derive(Clone)]
pub struct FastF1HistoricalClient {
    root: PathBuf,
    python_override: Option<PathBuf>,
    bootstrap_python: String,
    timeout: Duration,
}

impl Default for FastF1HistoricalClient {
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            python_override: std::env::var("INTERVAL_FASTF1_PYTHON")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from),
            bootstrap_python: std::env::var("INTERVAL_FASTF1_BOOTSTRAP_PYTHON")
                .unwrap_or_else(|_| "python".to_string()),
            timeout: Duration::from_secs(
                std::env::var("INTERVAL_FASTF1_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(900),
            ),
        }
    }
}

impl FastF1HistoricalClient {
    #[cfg(test)]
    pub fn for_test(root: PathBuf, python_override: Option<PathBuf>) -> Self {
        Self {
            root,
            python_override,
            bootstrap_python: "python".to_string(),
            timeout: Duration::from_secs(30),
        }
    }

    pub async fn fetch_race_bundle(
        &self,
        session: &Session,
    ) -> Result<Vec<RawEndpoint>, FastF1HistoricalError> {
        let config = FastF1SessionConfig::for_session(session)?;
        let root = self.root.clone();
        let python_override = self.python_override.clone();
        let bootstrap_python = self.bootstrap_python.clone();
        let timeout = self.timeout;
        let session_key = session.session_key;

        tokio::task::spawn_blocking(move || {
            let python = match python_override {
                Some(path) => path,
                None => ensure_managed_python(&root, &bootstrap_python)?,
            };
            let output_path = root
                .join("cache")
                .join("fastf1-bundles")
                .join(format!("{session_key}.json"));
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut command = Command::new(&python);
            command
                .current_dir(&root)
                .arg(SCRIPT_PATH)
                .arg("--year")
                .arg(config.year.to_string())
                .arg("--round")
                .arg(config.round.to_string())
                .arg("--session")
                .arg(config.session_code)
                .arg("--session-key")
                .arg(session_key.to_string())
                .arg("--cache-dir")
                .arg(DEFAULT_CACHE_DIR)
                .arg("--output")
                .arg(&output_path);

            let output = run_command(command, timeout)?;
            if !output.status.success() {
                return Err(FastF1HistoricalError::CommandFailed {
                    program: python.display().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                });
            }

            let payload = fs::read_to_string(output_path)?;
            bundle_from_export(session_key, &payload)
        })
        .await
        .map_err(|error| FastF1HistoricalError::Join(error.to_string()))?
    }
}

fn ensure_managed_python(
    root: &Path,
    bootstrap_python: &str,
) -> Result<PathBuf, FastF1HistoricalError> {
    let venv_dir = root.join(DEFAULT_VENV_DIR);
    let python = venv_python(&venv_dir);
    let sentinel = venv_dir.join(READY_SENTINEL);
    if sentinel.exists() && python.exists() {
        return Ok(python);
    }

    if !venv_dir.exists() {
        let mut create = Command::new(bootstrap_python);
        create
            .current_dir(root)
            .arg("-m")
            .arg("venv")
            .arg(&venv_dir);
        let output = run_command(create, Duration::from_secs(120))?;
        if !output.status.success() {
            return Err(FastF1HistoricalError::CommandFailed {
                program: bootstrap_python.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
    }

    let mut install = Command::new(&python);
    install
        .current_dir(root)
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-r")
        .arg(REQUIREMENTS_PATH);
    let output = run_command(install, Duration::from_secs(900))?;
    if !output.status.success() {
        return Err(FastF1HistoricalError::CommandFailed {
            program: python.display().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    fs::write(sentinel, "ready\n")?;
    Ok(python)
}

#[cfg(windows)]
fn venv_python(venv_dir: &Path) -> PathBuf {
    venv_dir.join("Scripts").join("python.exe")
}

#[cfg(not(windows))]
fn venv_python(venv_dir: &Path) -> PathBuf {
    venv_dir.join("bin").join("python")
}

fn run_command(
    mut command: Command,
    timeout: Duration,
) -> Result<std::process::Output, FastF1HistoricalError> {
    let started = std::time::Instant::now();
    let mut child = command.spawn()?;
    loop {
        if let Some(_status) = child.try_wait()? {
            return Ok(child.wait_with_output()?);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FastF1HistoricalError::TimedOut(timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn bundle_from_export(
    session_key: i64,
    payload: &str,
) -> Result<Vec<RawEndpoint>, FastF1HistoricalError> {
    let export = serde_json::from_str::<FastF1Export>(payload)?;
    let mut bundle = Vec::with_capacity(export.sections.len() + 1);
    bundle.push(RawEndpoint {
        endpoint: "fastf1_metadata".to_string(),
        session_key,
        payload: export.metadata.unwrap_or(Value::Object(Default::default())),
    });
    for (name, payload) in export.sections {
        bundle.push(RawEndpoint {
            endpoint: format!("fastf1_{name}"),
            session_key,
            payload,
        });
    }
    Ok(bundle)
}

#[derive(Debug)]
struct FastF1SessionConfig {
    year: i32,
    round: i32,
    session_code: &'static str,
}

impl FastF1SessionConfig {
    fn for_session(session: &Session) -> Result<Self, FastF1HistoricalError> {
        match session.session_key {
            9472 => Ok(Self {
                year: 2024,
                round: 1,
                session_code: "R",
            }),
            _ if session.name.eq_ignore_ascii_case("race") => {
                Err(FastF1HistoricalError::UnsupportedSession(format!(
                    "FastF1 session mapping is not configured for session_key {}",
                    session.session_key
                )))
            }
            _ => Err(FastF1HistoricalError::UnsupportedSession(
                "only race sessions are supported".to_string(),
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FastF1Export {
    #[serde(default)]
    metadata: Option<Value>,
    sections: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Error)]
pub enum FastF1HistoricalError {
    #[error("FastF1 session is unsupported: {0}")]
    UnsupportedSession(String),
    #[error("FastF1 command timed out after {0}s")]
    TimedOut(u64),
    #[error("FastF1 command failed ({program}): {stderr}")]
    CommandFailed { program: String, stderr: String },
    #[error("FastF1 process join failed: {0}")]
    Join(String),
    #[error("FastF1 filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("FastF1 export JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bahrain_session_maps_to_fastf1_round() {
        let session = Session {
            session_key: 9472,
            meeting_key: 1229,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: String::new(),
            end_time: String::new(),
            total_laps: 57,
        };

        let config = FastF1SessionConfig::for_session(&session).unwrap();

        assert_eq!(config.year, 2024);
        assert_eq!(config.round, 1);
        assert_eq!(config.session_code, "R");
    }

    #[test]
    fn export_sections_become_raw_cache_endpoints() {
        let payload = json!({
            "metadata": { "source": "fastf1_historical" },
            "sections": {
                "drivers": [{ "driver_number": 1 }],
                "telemetry": [{ "driver_number": 1, "t": 1.0 }]
            }
        })
        .to_string();

        let bundle = bundle_from_export(9472, &payload).unwrap();

        assert!(bundle
            .iter()
            .any(|entry| entry.endpoint == "fastf1_metadata"));
        assert!(bundle
            .iter()
            .any(|entry| entry.endpoint == "fastf1_drivers"));
        assert!(bundle
            .iter()
            .any(|entry| entry.endpoint == "fastf1_telemetry"));
    }
}
