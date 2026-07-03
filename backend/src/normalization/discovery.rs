use crate::domain::{Meeting, Session, SessionType};
use serde::Deserialize;
use serde_json::Value;

pub fn meetings_from_openf1(payload: Value) -> anyhow::Result<Vec<Meeting>> {
    let rows = serde_json::from_value::<Vec<OpenF1Meeting>>(payload)?;
    Ok(rows
        .into_iter()
        .map(|row| Meeting {
            meeting_key: row.meeting_key,
            year: row.year,
            name: row.meeting_name,
            country: row.country_name.unwrap_or_default(),
            location: row.location.unwrap_or_default(),
        })
        .collect())
}

pub fn race_sessions_from_openf1(payload: Value) -> anyhow::Result<Vec<Session>> {
    let rows = serde_json::from_value::<Vec<OpenF1Session>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let session_type_label = row.session_type.as_deref().unwrap_or_default();
            let session_name_label = row.session_name.as_deref().unwrap_or_default();
            let session_type = supported_session_type(session_type_label, session_name_label)?;
            let name = row
                .session_name
                .filter(|value| !value.trim().is_empty())
                .or(row.session_type)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| format!("Session {}", row.session_key));
            Some(Session {
                session_key: row.session_key,
                meeting_key: row.meeting_key,
                year: row.year,
                name,
                session_type,
                start_time: row.date_start.unwrap_or_default(),
                end_time: row.date_end.unwrap_or_default(),
                total_laps: 0,
            })
        })
        .collect())
}

fn supported_session_type(session_type: &str, session_name: &str) -> Option<SessionType> {
    let type_label = session_type.trim().to_lowercase();
    let name_label = session_name.trim().to_lowercase();
    if type_label == "race" || name_label == "race" {
        return Some(SessionType::Race);
    }
    if type_label == "sprint" || name_label == "sprint" || name_label == "sprint race" {
        return Some(SessionType::Sprint);
    }
    None
}

#[derive(Debug, Deserialize)]
struct OpenF1Meeting {
    meeting_key: i64,
    meeting_name: String,
    year: i32,
    country_name: Option<String>,
    location: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenF1Session {
    session_key: i64,
    meeting_key: i64,
    session_name: Option<String>,
    session_type: Option<String>,
    date_start: Option<String>,
    date_end: Option<String>,
    year: i32,
}
