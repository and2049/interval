use crate::domain::{
    Driver, Lap, RaceControlMessage, RankSource, Stint, TrackPositionQuality, TrackPositionSample,
    TrackPositionSource, TyreCompound, WeatherSample,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use super::{
    IntervalRecord, LapRecord, LocationRecord, PitEvent, PositionRecord, RaceData, RaceDataSource,
    SessionResult,
};

pub(super) fn race_data_from_bundle(
    bundle: &[crate::connectors::openf1_historical::RawEndpoint],
    _session: &crate::domain::Session,
) -> anyhow::Result<RaceData> {
    let by_endpoint = bundle
        .iter()
        .map(|entry| (entry.endpoint.as_str(), entry.payload.clone()))
        .collect::<HashMap<_, _>>();

    Ok(RaceData {
        source: RaceDataSource::FastF1Historical,
        drivers: drivers(payload(&by_endpoint, "fastf1_drivers"))?,
        laps: laps(payload(&by_endpoint, "fastf1_laps"))?,
        intervals: intervals(payload(&by_endpoint, "fastf1_intervals"))?,
        positions: positions(payload(&by_endpoint, "fastf1_positions"))?,
        locations: telemetry_locations(payload(&by_endpoint, "fastf1_telemetry"))?,
        geometry_locations: geometry_locations(
            by_endpoint
                .get("fastf1_geometry")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "centerline": [] })),
        )?,
        pits: pits(payload(&by_endpoint, "fastf1_pits"))?,
        race_control: race_control(payload(&by_endpoint, "fastf1_track_status"))?,
        stints: stints(payload(&by_endpoint, "fastf1_stints"))?,
        weather: weather(payload(&by_endpoint, "fastf1_weather"))?,
        session_results: session_results(payload(&by_endpoint, "fastf1_session_result"))?,
    })
}

fn payload(by_endpoint: &HashMap<&str, Value>, endpoint: &str) -> Value {
    by_endpoint
        .get(endpoint)
        .cloned()
        .unwrap_or(Value::Array(vec![]))
}

fn drivers(payload: Value) -> anyhow::Result<Vec<Driver>> {
    Ok(serde_json::from_value::<Vec<FastF1Driver>>(payload)?
        .into_iter()
        .map(|row| Driver {
            driver_number: row.driver_number,
            code: row.code.unwrap_or_else(|| row.driver_number.to_string()),
            full_name: row.full_name.unwrap_or_default(),
            team_name: row.team_name.unwrap_or_default(),
            team_colour: row.team_colour.unwrap_or_else(|| "7f8a99".to_string()),
        })
        .collect())
}

fn laps(payload: Value) -> anyhow::Result<Vec<LapRecord>> {
    Ok(serde_json::from_value::<Vec<FastF1Lap>>(payload)?
        .into_iter()
        .filter(|row| row.t_start.is_finite())
        .map(|row| LapRecord {
            t_start: row.t_start,
            lap: Lap {
                driver_number: row.driver_number,
                lap_number: row.lap_number,
                lap_duration: row.lap_duration,
                sector_1: row.sector_1,
                sector_2: row.sector_2,
                sector_3: row.sector_3,
                is_pit_out_lap: row.is_pit_out_lap.unwrap_or(false),
            },
        })
        .collect())
}

fn intervals(payload: Value) -> anyhow::Result<Vec<IntervalRecord>> {
    Ok(serde_json::from_value::<Vec<FastF1Interval>>(payload)?
        .into_iter()
        .filter(|row| row.t.is_finite())
        .map(|row| IntervalRecord {
            t: row.t,
            driver_number: row.driver_number,
            gap_to_leader: row.gap_to_leader,
            interval: row.interval,
        })
        .collect())
}

fn positions(payload: Value) -> anyhow::Result<Vec<PositionRecord>> {
    Ok(serde_json::from_value::<Vec<FastF1Position>>(payload)?
        .into_iter()
        .filter(|row| row.t.is_finite())
        .map(|row| PositionRecord {
            t: row.t,
            position: row.position,
            rank_source: RankSource::FastF1Position,
            sample: TrackPositionSample {
                driver_number: row.driver_number,
                x: 0.0,
                y: 0.0,
                z: None,
                relative_distance: None,
                source: TrackPositionSource::Schematic,
                quality: TrackPositionQuality::Missing,
                stale_seconds: None,
            },
        })
        .collect())
}

fn telemetry_locations(payload: Value) -> anyhow::Result<Vec<LocationRecord>> {
    Ok(serde_json::from_value::<Vec<FastF1Telemetry>>(payload)?
        .into_iter()
        .filter_map(|row| {
            if !row.t.is_finite() || !row.x.is_finite() || !row.y.is_finite() {
                return None;
            }
            Some(LocationRecord {
                t: row.t,
                driver_number: row.driver_number,
                x: row.x,
                y: row.y,
                z: row.z,
            })
        })
        .collect())
}

fn geometry_locations(payload: Value) -> anyhow::Result<Vec<LocationRecord>> {
    let geometry = serde_json::from_value::<FastF1Geometry>(payload)?;
    Ok(geometry
        .centerline
        .into_iter()
        .enumerate()
        .filter_map(|(idx, point)| {
            if point.len() < 2 || !point[0].is_finite() || !point[1].is_finite() {
                return None;
            }
            Some(LocationRecord {
                t: idx as f64,
                driver_number: 0,
                x: point[0],
                y: point[1],
                z: None,
            })
        })
        .collect())
}

fn pits(payload: Value) -> anyhow::Result<Vec<PitEvent>> {
    Ok(serde_json::from_value::<Vec<FastF1Pit>>(payload)?
        .into_iter()
        .filter(|row| row.t.is_finite())
        .map(|row| PitEvent {
            t: row.t,
            driver_number: row.driver_number,
            lap_number: row.lap_number,
            pit_duration: row.pit_duration,
        })
        .collect())
}

fn race_control(payload: Value) -> anyhow::Result<Vec<RaceControlMessage>> {
    Ok(serde_json::from_value::<Vec<FastF1RaceControl>>(payload)?
        .into_iter()
        .filter(|row| row.t.is_finite())
        .map(|row| RaceControlMessage {
            t: row.t,
            category: row.category.unwrap_or_else(|| "track_status".to_string()),
            message: row.message,
            flag: row.flag,
            scope: row.scope,
        })
        .collect())
}

fn stints(payload: Value) -> anyhow::Result<Vec<Stint>> {
    Ok(serde_json::from_value::<Vec<FastF1Stint>>(payload)?
        .into_iter()
        .map(|row| Stint {
            driver_number: row.driver_number,
            stint_number: row.stint_number.unwrap_or(0),
            compound: compound(row.compound.as_deref()),
            lap_start: row.lap_start,
            lap_end: row.lap_end,
            tyre_age_at_start: row.tyre_age_at_start,
        })
        .collect())
}

fn weather(payload: Value) -> anyhow::Result<Vec<WeatherSample>> {
    Ok(serde_json::from_value::<Vec<FastF1Weather>>(payload)?
        .into_iter()
        .filter(|row| row.t.is_finite())
        .map(|row| WeatherSample {
            t: row.t,
            air_temp: row.air_temp,
            track_temp: row.track_temp,
            humidity: row.humidity,
            rainfall: row.rainfall,
            wind_direction: row.wind_direction,
            wind_speed: row.wind_speed,
        })
        .collect())
}

fn session_results(payload: Value) -> anyhow::Result<Vec<SessionResult>> {
    Ok(serde_json::from_value::<Vec<FastF1SessionResult>>(payload)?
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
struct FastF1Driver {
    driver_number: i32,
    code: Option<String>,
    full_name: Option<String>,
    team_name: Option<String>,
    team_colour: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FastF1Lap {
    driver_number: i32,
    lap_number: i32,
    t_start: f64,
    lap_duration: Option<f64>,
    sector_1: Option<f64>,
    sector_2: Option<f64>,
    sector_3: Option<f64>,
    is_pit_out_lap: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct FastF1Interval {
    t: f64,
    driver_number: i32,
    gap_to_leader: Option<String>,
    interval: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FastF1Position {
    t: f64,
    driver_number: i32,
    position: i32,
}

#[derive(Debug, Deserialize)]
struct FastF1Telemetry {
    t: f64,
    driver_number: i32,
    x: f64,
    y: f64,
    z: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct FastF1Geometry {
    #[serde(default)]
    centerline: Vec<Vec<f64>>,
}

#[derive(Debug, Deserialize)]
struct FastF1Pit {
    t: f64,
    driver_number: i32,
    lap_number: Option<i32>,
    pit_duration: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct FastF1RaceControl {
    t: f64,
    message: String,
    category: Option<String>,
    flag: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FastF1Stint {
    driver_number: i32,
    stint_number: Option<i32>,
    compound: Option<String>,
    lap_start: i32,
    lap_end: Option<i32>,
    tyre_age_at_start: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct FastF1Weather {
    t: f64,
    air_temp: Option<f64>,
    track_temp: Option<f64>,
    humidity: Option<f64>,
    rainfall: Option<f64>,
    wind_direction: Option<i32>,
    wind_speed: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct FastF1SessionResult {
    driver_number: i32,
    position: Option<i32>,
    dnf: Option<bool>,
    dns: Option<bool>,
    dsq: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::openf1_historical::RawEndpoint;
    use serde_json::json;

    #[test]
    fn normalizes_fastf1_bundle_into_race_data() {
        let bundle = vec![
            raw(
                "fastf1_drivers",
                json!([{ "driver_number": 1, "code": "VER" }]),
            ),
            raw(
                "fastf1_laps",
                json!([{ "driver_number": 1, "lap_number": 1, "t_start": 222.3, "lap_duration": 91.1 }]),
            ),
            raw(
                "fastf1_telemetry",
                json!([{ "driver_number": 1, "t": 223.0, "x": 10.0, "y": 20.0 }]),
            ),
            raw(
                "fastf1_positions",
                json!([{ "driver_number": 1, "t": 223.0, "position": 1 }]),
            ),
            raw(
                "fastf1_geometry",
                json!({ "centerline": [[0.0, 0.0], [10.0, 0.0]] }),
            ),
        ];
        let session = crate::domain::Session {
            session_key: 9472,
            meeting_key: 1229,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: String::new(),
            end_time: String::new(),
            total_laps: 57,
        };

        let data = race_data_from_bundle(&bundle, &session).unwrap();

        assert_eq!(data.source, RaceDataSource::FastF1Historical);
        assert_eq!(data.drivers[0].code, "VER");
        assert_eq!(data.laps[0].t_start, 222.3);
        assert_eq!(data.locations[0].x, 10.0);
        assert_eq!(data.geometry_locations.len(), 2);
        assert_eq!(data.positions[0].position, 1);
    }

    fn raw(endpoint: &str, payload: Value) -> RawEndpoint {
        RawEndpoint {
            endpoint: endpoint.to_string(),
            session_key: 9472,
            payload,
        }
    }
}
