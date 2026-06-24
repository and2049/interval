mod location;

pub use location::{location_samples, LocationRecord};

use crate::{
    connectors::openf1_historical::RawEndpoint,
    domain::{
        Driver, Lap, Meeting, RaceControlMessage, Session, SessionType, Stint,
        TrackPositionQuality, TrackPositionSample, TrackPositionSource, TyreCompound,
        WeatherSample,
    },
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RaceData {
    pub drivers: Vec<Driver>,
    pub laps: Vec<LapRecord>,
    pub intervals: Vec<IntervalRecord>,
    pub positions: Vec<PositionRecord>,
    pub locations: Vec<LocationRecord>,
    pub pits: Vec<PitEvent>,
    pub race_control: Vec<RaceControlMessage>,
    pub stints: Vec<Stint>,
    pub weather: Vec<WeatherSample>,
    pub session_results: Vec<SessionResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LapRecord {
    pub lap: Lap,
    pub t_start: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntervalRecord {
    pub t: f64,
    pub driver_number: i32,
    pub gap_to_leader: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionRecord {
    pub t: f64,
    pub position: i32,
    pub sample: TrackPositionSample,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PitEvent {
    pub t: f64,
    pub driver_number: i32,
    pub lap_number: Option<i32>,
    pub pit_duration: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionResult {
    pub driver_number: i32,
    pub position: Option<i32>,
    pub dnf: bool,
    pub dns: bool,
    pub dsq: bool,
}

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

pub fn race_data_from_bundle(
    bundle: &[RawEndpoint],
    session: &Session,
) -> anyhow::Result<RaceData> {
    let by_endpoint = bundle
        .iter()
        .map(|entry| (entry.endpoint.as_str(), entry.payload.clone()))
        .collect::<HashMap<_, _>>();
    let session_start = parse_date(&session.start_time);

    Ok(RaceData {
        drivers: drivers(
            by_endpoint
                .get("drivers")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )?,
        laps: laps(
            by_endpoint
                .get("laps")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        intervals: intervals(
            by_endpoint
                .get("intervals")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        positions: positions(
            by_endpoint
                .get("position")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        locations: location_samples(
            by_endpoint
                .get("location")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        pits: pits(
            by_endpoint
                .get("pit")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        race_control: race_control(
            by_endpoint
                .get("race_control")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        stints: stints(
            by_endpoint
                .get("stints")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )?,
        weather: weather(
            by_endpoint
                .get("weather")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        session_results: session_results(
            by_endpoint
                .get("session_result")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )?,
    })
}

fn drivers(payload: Value) -> anyhow::Result<Vec<Driver>> {
    let rows = serde_json::from_value::<Vec<OpenF1Driver>>(payload)?;
    Ok(rows
        .into_iter()
        .map(|row| Driver {
            driver_number: row.driver_number,
            code: row
                .name_acronym
                .unwrap_or_else(|| row.driver_number.to_string()),
            full_name: row.full_name.unwrap_or_default(),
            team_name: row.team_name.unwrap_or_default(),
            team_colour: row.team_colour.unwrap_or_else(|| "7f8a99".to_string()),
        })
        .collect())
}

fn laps(payload: Value, session_start: Option<DateTime<Utc>>) -> anyhow::Result<Vec<LapRecord>> {
    let rows = serde_json::from_value::<Vec<OpenF1Lap>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(LapRecord {
                t_start: t_since_start(row.date_start.as_deref(), session_start)?,
                lap: Lap {
                    driver_number: row.driver_number,
                    lap_number: row.lap_number,
                    lap_duration: row.lap_duration,
                    sector_1: row.duration_sector_1,
                    sector_2: row.duration_sector_2,
                    sector_3: row.duration_sector_3,
                    is_pit_out_lap: row.is_pit_out_lap.unwrap_or(false),
                },
            })
        })
        .collect())
}

fn intervals(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<IntervalRecord>> {
    let rows = serde_json::from_value::<Vec<OpenF1Interval>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(IntervalRecord {
                t: t_since_start(row.date.as_deref(), session_start)?,
                driver_number: row.driver_number,
                gap_to_leader: display_interval(row.gap_to_leader),
                interval: display_interval(row.interval),
            })
        })
        .collect())
}

fn positions(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<PositionRecord>> {
    let rows = serde_json::from_value::<Vec<OpenF1PositionLike>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let t = t_since_start(row.date.as_deref(), session_start)?;
            Some(PositionRecord {
                t,
                position: row.position.unwrap_or(0),
                sample: TrackPositionSample {
                    driver_number: row.driver_number,
                    x: row.x.unwrap_or(0.0),
                    y: row.y.unwrap_or(0.0),
                    z: row.z,
                    relative_distance: None,
                    source: TrackPositionSource::Schematic,
                    quality: TrackPositionQuality::Missing,
                    stale_seconds: None,
                },
            })
        })
        .collect())
}

fn pits(payload: Value, session_start: Option<DateTime<Utc>>) -> anyhow::Result<Vec<PitEvent>> {
    let rows = serde_json::from_value::<Vec<OpenF1Pit>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(PitEvent {
                t: t_since_start(row.date.as_deref(), session_start)?,
                driver_number: row.driver_number,
                lap_number: row.lap_number,
                pit_duration: row.pit_duration,
            })
        })
        .collect())
}

fn race_control(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<RaceControlMessage>> {
    let rows = serde_json::from_value::<Vec<OpenF1RaceControl>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(RaceControlMessage {
                t: t_since_start(row.date.as_deref(), session_start)?,
                category: row.category.unwrap_or_else(|| "race_control".to_string()),
                message: row.message.unwrap_or_default(),
                flag: row.flag,
                scope: row.scope,
            })
        })
        .collect())
}

fn stints(payload: Value) -> anyhow::Result<Vec<Stint>> {
    let rows = serde_json::from_value::<Vec<OpenF1Stint>>(payload)?;
    Ok(rows
        .into_iter()
        .map(|row| Stint {
            driver_number: row.driver_number,
            stint_number: row.stint_number.unwrap_or(0),
            compound: compound(row.compound.as_deref()),
            lap_start: row.lap_start.unwrap_or(0),
            lap_end: row.lap_end,
            tyre_age_at_start: row.tyre_age_at_start,
        })
        .collect())
}

fn weather(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<WeatherSample>> {
    let rows = serde_json::from_value::<Vec<OpenF1Weather>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(WeatherSample {
                t: t_since_start(row.date.as_deref(), session_start)?,
                air_temp: row.air_temperature,
                track_temp: row.track_temperature,
                humidity: row.humidity,
                rainfall: row.rainfall,
                wind_direction: row.wind_direction,
                wind_speed: row.wind_speed,
            })
        })
        .collect())
}

fn session_results(payload: Value) -> anyhow::Result<Vec<SessionResult>> {
    let rows = serde_json::from_value::<Vec<OpenF1SessionResult>>(payload)?;
    Ok(rows
        .into_iter()
        .map(|row| SessionResult {
            driver_number: row.driver_number,
            position: row.position,
            dnf: row.dnf.unwrap_or(false),
            dns: row.dns.unwrap_or(false),
            dsq: row.dsq.unwrap_or(false),
        })
        .collect())
}

pub(crate) fn parse_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

pub(crate) fn t_since_start(
    value: Option<&str>,
    session_start: Option<DateTime<Utc>>,
) -> Option<f64> {
    let date = parse_date(value?)?;
    let start = session_start?;
    Some((date - start).num_milliseconds() as f64 / 1000.0)
}

fn display_interval(value: Option<IntervalValue>) -> Option<String> {
    match value? {
        IntervalValue::Number(number) => Some(format!("+{number:.3}")),
        IntervalValue::Text(text) => Some(text),
    }
}

fn compound(value: Option<&str>) -> TyreCompound {
    match value.unwrap_or_default().to_ascii_uppercase().as_str() {
        "SOFT" => TyreCompound::Soft,
        "MEDIUM" => TyreCompound::Medium,
        "HARD" => TyreCompound::Hard,
        "INTERMEDIATE" => TyreCompound::Intermediate,
        "WET" => TyreCompound::Wet,
        _ => TyreCompound::Unknown,
    }
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

#[derive(Debug, Deserialize)]
struct OpenF1Driver {
    driver_number: i32,
    full_name: Option<String>,
    name_acronym: Option<String>,
    team_colour: Option<String>,
    team_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenF1Lap {
    driver_number: i32,
    lap_number: i32,
    date_start: Option<String>,
    lap_duration: Option<f64>,
    duration_sector_1: Option<f64>,
    duration_sector_2: Option<f64>,
    duration_sector_3: Option<f64>,
    is_pit_out_lap: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OpenF1Interval {
    date: Option<String>,
    driver_number: i32,
    gap_to_leader: Option<IntervalValue>,
    interval: Option<IntervalValue>,
}

#[derive(Debug, Deserialize)]
struct OpenF1PositionLike {
    date: Option<String>,
    driver_number: i32,
    position: Option<i32>,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenF1Pit {
    date: Option<String>,
    driver_number: i32,
    lap_number: Option<i32>,
    pit_duration: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenF1RaceControl {
    date: Option<String>,
    category: Option<String>,
    message: Option<String>,
    flag: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenF1Stint {
    driver_number: i32,
    stint_number: Option<i32>,
    compound: Option<String>,
    lap_start: Option<i32>,
    lap_end: Option<i32>,
    tyre_age_at_start: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct OpenF1Weather {
    date: Option<String>,
    air_temperature: Option<f64>,
    track_temperature: Option<f64>,
    humidity: Option<f64>,
    rainfall: Option<f64>,
    wind_direction: Option<i32>,
    wind_speed: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenF1SessionResult {
    driver_number: i32,
    position: Option<i32>,
    dnf: Option<bool>,
    dns: Option<bool>,
    dsq: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum IntervalValue {
    Number(f64),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_meetings() {
        let meetings = meetings_from_openf1(json!([{
            "meeting_key": 1216,
            "meeting_name": "Belgian Grand Prix",
            "country_name": "Belgium",
            "location": "Spa-Francorchamps",
            "year": 2023
        }]))
        .unwrap();

        assert_eq!(meetings[0].meeting_key, 1216);
        assert_eq!(meetings[0].name, "Belgian Grand Prix");
    }

    #[test]
    fn filters_race_sessions() {
        let sessions = race_sessions_from_openf1(json!([
            {
                "session_key": 1,
                "meeting_key": 10,
                "session_name": "Qualifying",
                "session_type": "Qualifying",
                "date_start": "2023-01-01T12:00:00+00:00",
                "date_end": "2023-01-01T13:00:00+00:00",
                "year": 2023
            },
            {
                "session_key": 2,
                "meeting_key": 10,
                "session_name": "Race",
                "session_type": "Race",
                "date_start": "2023-01-02T12:00:00+00:00",
                "date_end": "2023-01-02T14:00:00+00:00",
                "year": 2023
            }
        ]))
        .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_key, 2);
    }

    #[test]
    fn normalizes_interval_strings_and_numbers() {
        let endpoint = RawEndpoint {
            endpoint: "intervals".to_string(),
            session_key: 2,
            payload: json!([
                {
                    "date": "2023-01-02T12:00:04+00:00",
                    "driver_number": 4,
                    "gap_to_leader": "+1 LAP",
                    "interval": 1.234
                }
            ]),
        };
        let session = Session {
            session_key: 2,
            meeting_key: 10,
            year: 2023,
            name: "Race".to_string(),
            session_type: SessionType::Race,
            start_time: "2023-01-02T12:00:00+00:00".to_string(),
            end_time: "2023-01-02T14:00:00+00:00".to_string(),
            total_laps: 0,
        };

        let data = race_data_from_bundle(&[endpoint], &session).unwrap();
        assert_eq!(data.intervals[0].t, 4.0);
        assert_eq!(data.intervals[0].gap_to_leader.as_deref(), Some("+1 LAP"));
        assert_eq!(data.intervals[0].interval.as_deref(), Some("+1.234"));
    }
}
