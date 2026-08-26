use crate::domain::{
    DataQuality, MapMode, RaceControlSection, RaceState, ReplayCursor, ReplaySnapshot,
    ReplayWeatherSection, Session, TimingSection, TrackGeometry, TrackGeometryQuality,
    TrackGeometrySource, TrackSection, REPLAY_CONTRACT_VERSION,
};

pub(crate) fn build_indexed_snapshot(
    session: &Session,
    index: &super::indexed_data::ReplayDataIndex<'_>,
    geometry: &TrackGeometry,
    t: f64,
    frame_index: i64,
) -> ReplaySnapshot {
    let lap = index.latest_lap_number(t);
    let positions = index.track_positions(geometry, t);
    let weather = index.latest_weather(t);
    let rows = index.timing_rows(t);
    let track_status = index.track_status(t);
    let race_control_messages = index.race_control_history(t);
    let derived_metrics = index.recent_pace_metrics(&rows, t);
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
        (
            TrackGeometryQuality::Ready,
            TrackGeometrySource::OpenF1Location | TrackGeometrySource::FastF1Telemetry,
        ) => MapMode::Gps,
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
            last_lap_status: Default::default(),
            best_lap: None,
            best_lap_status: Default::default(),
            compound: crate::domain::TyreCompound::Unknown,
            stint_age: None,
            sectors: vec![],
            in_pit: false,
            status: crate::domain::DriverStatus::OnTrack,
        }
    }
}
