use crate::domain::{
    Driver, Lap, RaceControlMessage, Stint, TrackPositionQuality, TrackPositionSample,
    TrackPositionSource, TyreCompound, WeatherSample,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::{t_since_start, IntervalRecord, LapRecord, PitEvent, PositionRecord, SessionResult};

pub(super) fn drivers(payload: Value) -> anyhow::Result<Vec<Driver>> {
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

pub(super) fn laps(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<LapRecord>> {
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

pub(super) fn intervals(
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

pub(super) fn positions(
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

pub(super) fn pits(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<PitEvent>> {
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

pub(super) fn race_control(
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

pub(super) fn stints(payload: Value) -> anyhow::Result<Vec<Stint>> {
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

pub(super) fn weather(
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

pub(super) fn session_results(payload: Value) -> anyhow::Result<Vec<SessionResult>> {
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
