use crate::{
    domain::{EventKind, EventSeverity, EventSource, ReplayEvent},
    normalization::RaceData,
};

pub fn generate_events(data: &RaceData) -> Vec<ReplayEvent> {
    let mut events = Vec::new();
    for (idx, event) in data.race_control.iter().enumerate() {
        events.push(ReplayEvent {
            id: format!("race-control-{idx}"),
            t: event.t,
            kind: EventKind::RaceControl,
            severity: if event.flag.as_deref() == Some("red") {
                EventSeverity::Critical
            } else if event.flag.is_some() {
                EventSeverity::Warning
            } else {
                EventSeverity::Info
            },
            driver_number: None,
            message: event.message.clone(),
            source: EventSource::OpenF1,
            payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        });
        if event.flag.is_some() {
            events.push(ReplayEvent {
                id: format!("track-status-{idx}"),
                t: event.t,
                kind: EventKind::TrackStatus,
                severity: EventSeverity::Notice,
                driver_number: None,
                message: event.flag.clone().unwrap_or_default(),
                source: EventSource::OpenF1,
                payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
            });
        }
    }
    for (idx, pit) in data.pits.iter().enumerate() {
        events.push(ReplayEvent {
            id: format!("pit-stop-{idx}"),
            t: pit.t,
            kind: EventKind::PitStop,
            severity: EventSeverity::Info,
            driver_number: Some(pit.driver_number),
            message: format!("Driver {} pit stop", pit.driver_number),
            source: EventSource::OpenF1,
            payload: serde_json::json!({
                "driver_number": pit.driver_number,
                "lap_number": pit.lap_number,
                "pit_duration": pit.pit_duration
            }),
        });
    }
    events.extend(super::derived_events::stint_change_events(data));
    events.extend(super::derived_events::leader_change_events(&data.positions));
    events.extend(super::derived_events::weather_change_events(&data.weather));
    events.sort_by(|a, b| a.t.total_cmp(&b.t).then_with(|| a.id.cmp(&b.id)));
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            EventKind, EventSeverity, Stint, TrackPositionQuality, TrackPositionSample,
            TrackPositionSource, TyreCompound, WeatherSample,
        },
        normalization::{LapRecord, PitEvent, PositionRecord, SessionResult},
    };

    #[test]
    fn includes_derived_stint_leader_and_weather_changes() {
        let data = RaceData {
            drivers: vec![],
            laps: vec![lap_record(1, 10, 90.0), lap_record(4, 10, 95.0)],
            intervals: vec![],
            positions: vec![
                position_record(5.0, 1, 1),
                position_record(10.0, 4, 1),
                position_record(15.0, 4, 1),
            ],
            locations: vec![],
            pits: Vec::<PitEvent>::new(),
            race_control: vec![],
            stints: vec![
                Stint {
                    driver_number: 1,
                    stint_number: 1,
                    compound: TyreCompound::Medium,
                    lap_start: 1,
                    lap_end: Some(9),
                    tyre_age_at_start: Some(0),
                },
                Stint {
                    driver_number: 1,
                    stint_number: 2,
                    compound: TyreCompound::Hard,
                    lap_start: 10,
                    lap_end: None,
                    tyre_age_at_start: Some(0),
                },
            ],
            weather: vec![
                weather_sample(0.0, 30.0, 0.0),
                weather_sample(120.0, 32.5, 0.0),
                weather_sample(180.0, 32.5, 1.0),
            ],
            session_results: Vec::<SessionResult>::new(),
        };

        let events = generate_events(&data);
        assert!(events
            .iter()
            .any(|event| event.kind == EventKind::StintChange
                && event.driver_number == Some(1)
                && event.t == 90.0));
        assert!(events
            .iter()
            .any(|event| event.kind == EventKind::LeaderChange
                && event.driver_number == Some(4)
                && event.t == 10.0));
        assert!(events
            .iter()
            .any(|event| event.kind == EventKind::WeatherChange && event.t == 120.0));
        assert!(events
            .iter()
            .any(|event| event.kind == EventKind::WeatherChange
                && event.t == 180.0
                && event.severity == EventSeverity::Warning));
    }

    fn lap_record(driver_number: i32, lap_number: i32, t_start: f64) -> LapRecord {
        LapRecord {
            t_start,
            lap: crate::domain::Lap {
                driver_number,
                lap_number,
                lap_duration: Some(91.0),
                sector_1: None,
                sector_2: None,
                sector_3: None,
                is_pit_out_lap: false,
            },
        }
    }

    fn position_record(t: f64, driver_number: i32, position: i32) -> PositionRecord {
        PositionRecord {
            t,
            position,
            sample: TrackPositionSample {
                driver_number,
                x: 0.0,
                y: 0.0,
                z: None,
                relative_distance: None,
                source: TrackPositionSource::Schematic,
                quality: TrackPositionQuality::Missing,
                stale_seconds: None,
            },
        }
    }

    fn weather_sample(t: f64, track_temp: f64, rainfall: f64) -> WeatherSample {
        WeatherSample {
            t,
            air_temp: Some(20.0),
            track_temp: Some(track_temp),
            humidity: Some(40.0),
            rainfall: Some(rainfall),
            wind_direction: None,
            wind_speed: None,
        }
    }
}
