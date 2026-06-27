use crate::{
    connectors::openf1_historical::RawEndpoint,
    domain::{Meeting, Session, SessionType},
};
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
        meeting: Option<&Meeting>,
    ) -> Result<Vec<RawEndpoint>, FastF1HistoricalError> {
        let config = FastF1SessionConfig::for_session(session, meeting)?;
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
            let warnings = config.warnings.clone();
            command
                .current_dir(&root)
                .arg(SCRIPT_PATH)
                .arg("--year")
                .arg(config.year.to_string())
                .arg("--session")
                .arg(config.session_code)
                .arg("--session-key")
                .arg(session_key.to_string())
                .arg("--cache-dir")
                .arg(DEFAULT_CACHE_DIR)
                .arg("--output")
                .arg(&output_path)
                .arg("--resolver-method")
                .arg(config.resolver_method);

            match config.round {
                Some(round) => {
                    command.arg("--round").arg(round.to_string());
                }
                None => {
                    if let Some(event_name) = config.event_name {
                        command.arg("--event-name").arg(event_name);
                    }
                    if let Some(country) = config.country {
                        command.arg("--country").arg(country);
                    }
                    if let Some(location) = config.location {
                        command.arg("--location").arg(location);
                    }
                    if let Some(session_start) = config.session_start {
                        command.arg("--session-start").arg(session_start);
                    }
                }
            }

            let output = run_command(command, timeout)?;
            if !output.status.success() {
                return Err(FastF1HistoricalError::CommandFailed {
                    program: python.display().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                });
            }

            let payload = fs::read_to_string(output_path)?;
            bundle_from_export(session_key, &payload, &warnings)
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
    additional_warnings: &[String],
) -> Result<Vec<RawEndpoint>, FastF1HistoricalError> {
    let export = serde_json::from_str::<FastF1Export>(payload)?;
    let mut bundle = Vec::with_capacity(export.sections.len() + 1);
    let metadata = merge_metadata_warnings(
        export.metadata.unwrap_or(Value::Object(Default::default())),
        additional_warnings,
    );
    bundle.push(RawEndpoint {
        endpoint: "fastf1_metadata".to_string(),
        session_key,
        payload: metadata,
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

fn merge_metadata_warnings(mut metadata: Value, additional_warnings: &[String]) -> Value {
    if additional_warnings.is_empty() {
        return metadata;
    }
    if !metadata.is_object() {
        metadata = Value::Object(Default::default());
    }
    let object = metadata.as_object_mut().expect("metadata object");
    let warnings = object
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !warnings.is_array() {
        *warnings = Value::Array(Vec::new());
    }
    let array = warnings.as_array_mut().expect("warnings array");
    for warning in additional_warnings {
        array.push(Value::String(warning.clone()));
    }
    metadata
}

#[derive(Debug)]
struct FastF1SessionConfig {
    year: i32,
    round: Option<i32>,
    session_code: &'static str,
    resolver_method: &'static str,
    event_name: Option<String>,
    country: Option<String>,
    location: Option<String>,
    session_start: Option<String>,
    warnings: Vec<String>,
}

impl FastF1SessionConfig {
    fn for_session(
        session: &Session,
        meeting: Option<&Meeting>,
    ) -> Result<Self, FastF1HistoricalError> {
        match (session.session_key, &session.session_type) {
            (9472, SessionType::Race) => Ok(Self {
                year: 2024,
                round: Some(1),
                session_code: "R",
                resolver_method: "curated_override",
                event_name: None,
                country: None,
                location: None,
                session_start: None,
                warnings: vec![
                    "FastF1 resolver used curated override for session_key 9472.".to_string(),
                ],
            }),
            (_, SessionType::Race | SessionType::Sprint) => match meeting {
                Some(meeting) => Ok(Self {
                    year: session.year,
                    round: None,
                    session_code: match session.session_type {
                        SessionType::Race => "R",
                        SessionType::Sprint => "S",
                    },
                    resolver_method: "fastf1_schedule_match",
                    event_name: Some(meeting.name.clone()),
                    country: Some(meeting.country.clone()),
                    location: Some(meeting.location.clone()),
                    session_start: Some(session.start_time.clone()),
                    warnings: vec![],
                }),
                None => Err(FastF1HistoricalError::UnsupportedSession(format!(
                    "meeting metadata is required to resolve session_key {} with FastF1",
                    session.session_key
                ))),
            },
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

        let config = FastF1SessionConfig::for_session(&session, None).unwrap();

        assert_eq!(config.year, 2024);
        assert_eq!(config.round, Some(1));
        assert_eq!(config.session_code, "R");
        assert_eq!(config.resolver_method, "curated_override");
    }

    #[test]
    fn race_session_uses_meeting_metadata_for_schedule_match() {
        let session = Session {
            session_key: 9999,
            meeting_key: 2222,
            year: 2025,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2025-04-06T05:00:00Z".to_string(),
            end_time: String::new(),
            total_laps: 53,
        };
        let meeting = Meeting {
            meeting_key: 2222,
            year: 2025,
            name: "Japanese Grand Prix".to_string(),
            country: "Japan".to_string(),
            location: "Suzuka".to_string(),
        };

        let config = FastF1SessionConfig::for_session(&session, Some(&meeting)).unwrap();

        assert_eq!(config.year, 2025);
        assert_eq!(config.round, None);
        assert_eq!(config.session_code, "R");
        assert_eq!(config.resolver_method, "fastf1_schedule_match");
        assert_eq!(config.event_name.as_deref(), Some("Japanese Grand Prix"));
        assert_eq!(config.country.as_deref(), Some("Japan"));
        assert_eq!(config.location.as_deref(), Some("Suzuka"));
    }

    #[test]
    fn another_race_session_uses_schedule_match_without_bahrain_override() {
        let session = Session {
            session_key: 10_100,
            meeting_key: 2_300,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2024-09-01T13:00:00Z".to_string(),
            end_time: String::new(),
            total_laps: 53,
        };
        let meeting = Meeting {
            meeting_key: 2_300,
            year: 2024,
            name: "Italian Grand Prix".to_string(),
            country: "Italy".to_string(),
            location: "Monza".to_string(),
        };

        let config = FastF1SessionConfig::for_session(&session, Some(&meeting)).unwrap();

        assert_eq!(config.year, 2024);
        assert_eq!(config.round, None);
        assert_eq!(config.resolver_method, "fastf1_schedule_match");
        assert_eq!(config.event_name.as_deref(), Some("Italian Grand Prix"));
        assert_eq!(config.country.as_deref(), Some("Italy"));
        assert_eq!(config.location.as_deref(), Some("Monza"));
    }

    #[test]
    fn sprint_session_maps_to_fastf1_sprint_code() {
        let session = Session {
            session_key: 20_100,
            meeting_key: 2_400,
            year: 2024,
            name: "Sprint".to_string(),
            session_type: crate::domain::SessionType::Sprint,
            start_time: "2024-05-04T16:00:00Z".to_string(),
            end_time: String::new(),
            total_laps: 19,
        };
        let meeting = Meeting {
            meeting_key: 2_400,
            year: 2024,
            name: "Miami Grand Prix".to_string(),
            country: "United States".to_string(),
            location: "Miami".to_string(),
        };

        let config = FastF1SessionConfig::for_session(&session, Some(&meeting)).unwrap();

        assert_eq!(config.year, 2024);
        assert_eq!(config.round, None);
        assert_eq!(config.session_code, "S");
        assert_eq!(config.resolver_method, "fastf1_schedule_match");
        assert_eq!(config.event_name.as_deref(), Some("Miami Grand Prix"));
    }

    #[test]
    fn schedule_match_requires_meeting_metadata() {
        let session = Session {
            session_key: 9999,
            meeting_key: 2222,
            year: 2025,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: String::new(),
            end_time: String::new(),
            total_laps: 53,
        };

        let error = FastF1SessionConfig::for_session(&session, None).unwrap_err();

        assert!(error.to_string().contains("meeting metadata is required"));
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

        let bundle = bundle_from_export(9472, &payload, &[]).unwrap();

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

    #[test]
    fn resolver_warnings_are_merged_into_metadata() {
        let payload = json!({
            "metadata": { "source": "fastf1_historical", "warnings": ["python warning"] },
            "sections": {}
        })
        .to_string();

        let bundle = bundle_from_export(9472, &payload, &["rust warning".to_string()]).unwrap();
        let metadata = bundle
            .iter()
            .find(|entry| entry.endpoint == "fastf1_metadata")
            .unwrap();

        assert_eq!(
            metadata.payload["warnings"],
            json!(["python warning", "rust warning"])
        );
    }
}
