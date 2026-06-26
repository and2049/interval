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
        .filter(|row| row.session_type.eq_ignore_ascii_case("race"))
        .map(|row| Session {
            session_key: row.session_key,
            meeting_key: row.meeting_key,
            year: row.year,
            name: row.session_name,
            session_type: SessionType::Race,
            start_time: row.date_start.unwrap_or_default(),
            end_time: row.date_end.unwrap_or_default(),
            total_laps: 0,
        })
        .collect())
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
    session_name: String,
    session_type: String,
    date_start: Option<String>,
    date_end: Option<String>,
    year: i32,
}
