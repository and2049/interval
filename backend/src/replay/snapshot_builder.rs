use crate::{
    domain::{
        DataQuality, MapMode, RaceControlMessage, RaceControlSection, RaceState, ReplayCursor,
        ReplaySnapshot, ReplayWeatherSection, Session, TimingSection, TrackGeometry,
        TrackGeometryQuality, TrackGeometrySource, TrackSection, WeatherSample,
        REPLAY_CONTRACT_VERSION,
    },
    normalization::RaceData,
};

pub fn build_snapshot(
    session: &Session,
    data: &RaceData,
    geometry: &TrackGeometry,
    t: f64,
    frame_index: i64,
) -> ReplaySnapshot {
    let lap = super::timing::latest_lap_number(data, t);
    let positions = super::track_positions::latest_positions(data, geometry, t);
    let weather = latest_weather(&data.weather, t);
    let rows = super::timing::timing_rows(data, t);
    let track_status = track_status(&data.race_control, t);
    let race_control_messages = race_control_history(&data.race_control, t);
    let derived_metrics = crate::analytics::recent_pace_metrics(&rows, &data.laps, t);
    let weather_quality = if weather.is_some() {
        DataQuality::Ready
    } else {
        DataQuality::Missing
    };
    let race_control_quality = if race_control_messages.is_empty() {
        DataQuality::Missing
    } else {
        DataQuality::Ready
    };

    ReplaySnapshot {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        cursor: ReplayCursor {
            session_key: session.session_key,
            t,
            frame_index,
            playback_speed: 1.0,
            is_paused: frame_index == 0,
        },
        race_state: RaceState { lap, track_status },
        timing: TimingSection {
            quality: timing_quality(&rows),
            rows,
        },
        track: TrackSection {
            positions,
            map_mode: map_mode(geometry),
            quality: track_quality(geometry),
        },
        weather: ReplayWeatherSection {
            sample: weather,
            quality: weather_quality,
        },
        race_control: RaceControlSection {
            messages: race_control_messages,
            quality: race_control_quality,
        },
        derived_metrics,
    }
}

fn map_mode(geometry: &TrackGeometry) -> MapMode {
    match (&geometry.quality, &geometry.source) {
        (TrackGeometryQuality::Ready, TrackGeometrySource::OpenF1Location) => MapMode::Gps,
        (TrackGeometryQuality::Ready, TrackGeometrySource::CuratedStatic) => MapMode::Projected,
        (TrackGeometryQuality::Ready, TrackGeometrySource::Schematic)
        | (TrackGeometryQuality::Schematic | TrackGeometryQuality::Missing, _) => {
            MapMode::Schematic
        }
    }
}

fn track_quality(geometry: &TrackGeometry) -> DataQuality {
    match map_mode(geometry) {
        MapMode::Gps => DataQuality::Ready,
        MapMode::Projected => DataQuality::Projected,
        MapMode::Schematic => match geometry.quality {
            TrackGeometryQuality::Missing => DataQuality::Missing,
            TrackGeometryQuality::Ready | TrackGeometryQuality::Schematic => DataQuality::Schematic,
        },
    }
}

fn timing_quality(rows: &[crate::domain::DriverSnapshot]) -> DataQuality {
    if rows.is_empty()
        || rows
            .iter()
            .all(|row| row.rank_source == crate::domain::RankSource::FallbackGrid)
    {
        DataQuality::Missing
    } else {
        DataQuality::Ready
    }
}

fn latest_weather(weather: &[WeatherSample], t: f64) -> Option<WeatherSample> {
    weather
        .iter()
        .filter(|sample| sample.t <= t)
        .max_by(|a, b| a.t.total_cmp(&b.t))
        .cloned()
}

fn track_status(events: &[RaceControlMessage], t: f64) -> String {
    events
        .iter()
        .filter(|event| event.t <= t)
        .filter(|event| event.flag.is_some())
        .max_by(|a, b| a.t.total_cmp(&b.t))
        .and_then(|event| event.flag.clone())
        .unwrap_or_else(|| "green".to_string())
}

fn race_control_history(events: &[RaceControlMessage], t: f64) -> Vec<RaceControlMessage> {
    let mut history = events
        .iter()
        .filter(|event| event.t <= t)
        .cloned()
        .collect::<Vec<_>>();
    history.sort_by(|a, b| a.t.total_cmp(&b.t).then_with(|| a.message.cmp(&b.message)));
    history
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_quality_follows_snapshot_map_mode() {
        let openf1 = TrackGeometry {
            contract_version: REPLAY_CONTRACT_VERSION.to_string(),
            session_key: 1,
            bounds: crate::domain::TrackBounds {
                min_x: 0.0,
                max_x: 1.0,
                min_y: 0.0,
                max_y: 1.0,
            },
            centerline: vec![],
            inner_edge: vec![],
            outer_edge: vec![],
            source: TrackGeometrySource::OpenF1Location,
            quality: TrackGeometryQuality::Ready,
            map_mode: MapMode::Gps,
            circuit_length: None,
            generated_at: String::new(),
        };
        let curated = TrackGeometry {
            source: TrackGeometrySource::CuratedStatic,
            map_mode: MapMode::Projected,
            ..openf1.clone()
        };
        let schematic = TrackGeometry {
            source: TrackGeometrySource::Schematic,
            quality: TrackGeometryQuality::Schematic,
            map_mode: MapMode::Schematic,
            ..openf1.clone()
        };

        assert_eq!(track_quality(&openf1), DataQuality::Ready);
        assert_eq!(track_quality(&curated), DataQuality::Projected);
        assert_eq!(track_quality(&schematic), DataQuality::Schematic);
    }

    #[test]
    fn timing_quality_distinguishes_ready_from_fallback_only_rows() {
        assert_eq!(timing_quality(&[]), DataQuality::Missing);
        assert_eq!(
            timing_quality(&[driver_snapshot(crate::domain::RankSource::FallbackGrid)]),
            DataQuality::Missing
        );
        assert_eq!(
            timing_quality(&[driver_snapshot(crate::domain::RankSource::OpenF1Position)]),
            DataQuality::Ready
        );
    }

    #[test]
    fn track_status_uses_latest_flag_by_timestamp() {
        let events = vec![
            race_control(120.0, Some("green"), "green flag"),
            race_control(60.0, Some("yellow"), "yellow flag"),
            race_control(90.0, None, "message only"),
        ];

        assert_eq!(track_status(&events, 100.0), "yellow");
        assert_eq!(track_status(&events, 130.0), "green");
    }

    #[test]
    fn race_control_history_is_ordered_by_event_time() {
        let events = vec![
            race_control(120.0, Some("green"), "green flag"),
            race_control(60.0, Some("yellow"), "yellow flag"),
            race_control(90.0, None, "message only"),
        ];

        let history = race_control_history(&events, 120.0);

        assert_eq!(
            history
                .iter()
                .map(|event| event.message.as_str())
                .collect::<Vec<_>>(),
            vec!["yellow flag", "message only", "green flag"]
        );
    }

    fn race_control(t: f64, flag: Option<&str>, message: &str) -> RaceControlMessage {
        RaceControlMessage {
            t,
            category: "race_control".to_string(),
            message: message.to_string(),
            flag: flag.map(str::to_string),
            scope: None,
        }
    }

    fn driver_snapshot(rank_source: crate::domain::RankSource) -> crate::domain::DriverSnapshot {
        crate::domain::DriverSnapshot {
            driver: crate::domain::Driver {
                driver_number: 1,
                code: "VER".to_string(),
                full_name: "Max Verstappen".to_string(),
                team_name: "Red Bull Racing".to_string(),
                team_colour: "3671C6".to_string(),
            },
            position: 1,
            rank_source,
            gap_to_leader: None,
            interval: None,
            lap: 1,
            last_lap: None,
            compound: crate::domain::TyreCompound::Unknown,
            stint_age: None,
            sectors: vec![],
            in_pit: false,
            status: crate::domain::DriverStatus::OnTrack,
        }
    }
}
