use super::track_status::{self, TrackStatusState};
use crate::{
    domain::{EventKind, EventSeverity, EventSource, ReplayEvent},
    normalization::RaceData,
};

pub fn generate_events(data: &RaceData) -> Vec<ReplayEvent> {
    let mut events = Vec::new();
    let source = event_source(data.source);
    // Track status is a fold over the history, so walk it in time order and emit one
    // TrackStatus event per change rather than one per flagged row: OpenF1 flags every
    // sector yellow and every blue flag, none of which is a track-status change.
    let mut race_control: Vec<&_> = data.race_control.iter().collect();
    race_control.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut status = TrackStatusState::default();
    let mut last_status = status.status();
    for event in race_control {
        let upper = event.message.to_ascii_uppercase();
        // Ids are content-derived so they stay stable across live refreshes
        // even when older rows get trimmed; SSE clients dedupe by id.
        events.push(ReplayEvent {
            id: format!(
                "race-control-{:.3}-{:08x}",
                event.t,
                stable_hash(&event.message)
            ),
            t: event.t,
            kind: EventKind::RaceControl,
            severity: if event.flag.as_deref() == Some("red") || track_status::mentions_red_flag(&upper) {
                EventSeverity::Critical
            } else if event.flag.is_some() || event.category.eq_ignore_ascii_case("SafetyCar") {
                EventSeverity::Warning
            } else {
                EventSeverity::Info
            },
            driver_number: None,
            message: event.message.clone(),
            source: source.clone(),
            payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        });
        status.apply(event);
        let current = status.status();
        if current != last_status {
            events.push(ReplayEvent {
                id: format!("track-status-{:.3}-{current}", event.t),
                t: event.t,
                kind: EventKind::TrackStatus,
                severity: if current == track_status::RED {
                    EventSeverity::Critical
                } else if current == track_status::GREEN {
                    EventSeverity::Notice
                } else {
                    EventSeverity::Warning
                },
                driver_number: None,
                message: current.clone(),
                source: source.clone(),
                payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
            });
            last_status = current;
        }
    }
    for pit in &data.pits {
        events.push(ReplayEvent {
            id: format!("pit-stop-{:.3}-{}", pit.t, pit.driver_number),
            t: pit.t,
            kind: EventKind::PitStop,
            severity: EventSeverity::Info,
            driver_number: Some(pit.driver_number),
            message: format!("Driver {} pit stop", pit.driver_number),
            source: source.clone(),
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
    events.extend(super::derived_events::data_gap_events(&data.locations));
    events.extend(super::derived_events::driver_out_events(data));
    events.sort_by(|a, b| a.t.total_cmp(&b.t).then_with(|| a.id.cmp(&b.id)));
    events
}

fn stable_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn event_source(source: crate::normalization::RaceDataSource) -> EventSource {
    match source {
        crate::normalization::RaceDataSource::FastF1Historical => EventSource::FastF1,
        crate::normalization::RaceDataSource::OpenF1Historical
        | crate::normalization::RaceDataSource::OpenF1Live => EventSource::OpenF1,
        crate::normalization::RaceDataSource::Demo => EventSource::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            EventKind, EventSeverity, Stint, TrackPositionQuality, TrackPositionSample,
            TrackPositionSource, TyreCompound, WeatherSample,
        },
        normalization::{LapRecord, LocationRecord, PitEvent, PositionRecord, SessionResult},
    };

    #[test]
    fn includes_derived_stint_leader_and_weather_changes() {
        let data = RaceData {
            source: crate::normalization::RaceDataSource::OpenF1Historical,
            drivers: vec![],
            laps: vec![lap_record(1, 10, 90.0), lap_record(4, 10, 95.0)],
            intervals: vec![],
            positions: vec![
                position_record(5.0, 1, 1),
                position_record(10.0, 4, 1),
                position_record(15.0, 4, 1),
            ],
            locations: vec![],
            geometry_locations: vec![],
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

    #[test]
    fn includes_data_gap_and_driver_out_events() {
        let data = RaceData {
            source: crate::normalization::RaceDataSource::FastF1Historical,
            drivers: vec![],
            laps: vec![],
            intervals: vec![],
            positions: vec![],
            locations: vec![
                location_record(0.0, 4),
                location_record(15.0, 4),
                location_record(20.0, 81),
            ],
            geometry_locations: vec![],
            pits: Vec::<PitEvent>::new(),
            race_control: vec![],
            stints: vec![],
            weather: vec![],
            session_results: vec![SessionResult {
                driver_number: 81,
                position: Some(18),
                dnf: true,
                dns: false,
                dsq: false,
            }],
        };

        let events = generate_events(&data);

        assert!(events.iter().any(|event| {
            event.kind == EventKind::DataGap && event.driver_number == Some(4) && event.t == 10.0
        }));
        assert!(events.iter().any(|event| {
            event.kind == EventKind::DriverOut
                && event.driver_number == Some(81)
                && event.source == EventSource::FastF1
                && event.t == 22.0
        }));
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
            rank_source: crate::domain::RankSource::OpenF1Position,
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

    fn location_record(t: f64, driver_number: i32) -> LocationRecord {
        LocationRecord {
            t,
            driver_number,
            x: t,
            y: 0.0,
            z: None,
            relative_distance: Some((t / 100.0).rem_euclid(1.0)),
        }
    }
}
