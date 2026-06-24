use super::{AvailableChannels, Meeting, Session, TrackGeometryQuality, TrackGeometrySource};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    NotIngested,
    Fetching,
    Normalizing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSupportStatus {
    Supported,
    Future,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSupport {
    pub status: SessionSupportStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionReadiness {
    pub session: Session,
    pub ingest_status: IngestStatus,
    pub replay_ready: bool,
    pub is_demo: bool,
    pub last_error: Option<String>,
    pub support_status: SessionSupportStatus,
    pub support_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointCoverage {
    pub endpoint: String,
    pub present: bool,
    pub rows: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestTrackGeometrySummary {
    pub status: TrackGeometryQuality,
    pub source: TrackGeometrySource,
    pub quality: TrackGeometryQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IngestResponse {
    pub session_key: i64,
    pub status: IngestStatus,
    pub cached_endpoints: usize,
    pub endpoint_coverage: Vec<EndpointCoverage>,
    pub generated_snapshots: usize,
    pub track_geometry: Option<IngestTrackGeometrySummary>,
    pub available_channels: Option<AvailableChannels>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

struct CancelledSessionRule {
    year: i32,
    name_tokens: &'static [&'static str],
    location_tokens: &'static [&'static str],
}

// TODO: Replace this curated list with a session availability resolver that compares
// OpenF1 discovery rows against the FastF1 schedule/cache before the UI enables ingest.
// This list only covers known cancelled events that OpenF1 can still expose as scheduled.
const CANCELLED_SESSION_RULES: &[CancelledSessionRule] = &[
    CancelledSessionRule {
        year: 2026,
        name_tokens: &["bahrain"],
        location_tokens: &["bahrain", "sakhir"],
    },
    CancelledSessionRule {
        year: 2026,
        name_tokens: &["saudi"],
        location_tokens: &["saudi", "jeddah"],
    },
];

pub fn session_support(
    session: &Session,
    meeting: Option<&Meeting>,
    now: DateTime<Utc>,
) -> SessionSupport {
    if is_known_cancelled_session(session, meeting) {
        return SessionSupport {
            status: SessionSupportStatus::Cancelled,
            reason: Some(
                "This event was cancelled, so FastF1 historical replay data is unavailable."
                    .to_string(),
            ),
        };
    }

    if let Ok(start) = DateTime::parse_from_rfc3339(&session.start_time) {
        if start.with_timezone(&Utc) > now {
            return SessionSupport {
                status: SessionSupportStatus::Future,
                reason: Some(
                    "This session has not happened yet, so historical replay data is unavailable."
                        .to_string(),
                ),
            };
        }
    }

    SessionSupport {
        status: SessionSupportStatus::Supported,
        reason: None,
    }
}

fn is_known_cancelled_session(session: &Session, meeting: Option<&Meeting>) -> bool {
    let Some(meeting) = meeting else {
        return false;
    };

    CANCELLED_SESSION_RULES.iter().any(|rule| {
        session.year == rule.year
            && contains_any(&meeting.name, rule.name_tokens)
            && (contains_any(&meeting.country, rule.location_tokens)
                || contains_any(&meeting.location, rule.location_tokens))
    })
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    let normalized = value.to_ascii_lowercase();
    needles.iter().any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_known_cancelled_sessions_unavailable() {
        let session = session("2026-04-19T17:00:00Z");
        let meeting = Meeting {
            meeting_key: 1,
            year: 2026,
            name: "Saudi Arabian Grand Prix".to_string(),
            country: "Saudi Arabia".to_string(),
            location: "Jeddah".to_string(),
        };

        let support = session_support(&session, Some(&meeting), now());

        assert_eq!(support.status, SessionSupportStatus::Cancelled);
        assert!(support
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("cancelled")));
    }

    #[test]
    fn marks_2026_bahrain_cancelled() {
        let session = session("2026-04-12T15:00:00Z");
        let meeting = Meeting {
            meeting_key: 2,
            year: 2026,
            name: "Bahrain Grand Prix".to_string(),
            country: "Bahrain".to_string(),
            location: "Sakhir".to_string(),
        };

        let support = session_support(&session, Some(&meeting), now());

        assert_eq!(support.status, SessionSupportStatus::Cancelled);
    }

    #[test]
    fn marks_future_sessions_unavailable() {
        let support = session_support(&session("2027-04-19T17:00:00Z"), None, now());

        assert_eq!(support.status, SessionSupportStatus::Future);
    }

    #[test]
    fn keeps_past_sessions_supported() {
        let support = session_support(&session("2024-03-02T15:00:00Z"), None, now());

        assert_eq!(support.status, SessionSupportStatus::Supported);
        assert_eq!(support.reason, None);
    }

    fn session(start_time: &str) -> Session {
        Session {
            session_key: 1,
            meeting_key: 1,
            year: 2026,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: start_time.to_string(),
            end_time: String::new(),
            total_laps: 0,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-27T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
}
