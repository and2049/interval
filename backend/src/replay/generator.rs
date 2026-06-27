use crate::{
    domain::{ReplayEvent, ReplayMetadata, ReplaySnapshot, Session, TrackGeometry},
    normalization::RaceData,
};

const SNAPSHOT_STEP_SECONDS: f64 = 0.5;
const DEFAULT_DURATION_SECONDS: f64 = 7_200.0;

pub struct GeneratedReplay {
    pub metadata: ReplayMetadata,
    pub snapshots: Vec<ReplaySnapshot>,
    pub events: Vec<ReplayEvent>,
    pub session: Session,
    pub track_geometry: TrackGeometry,
}

pub fn generate_replay(mut session: Session, data: RaceData) -> anyhow::Result<GeneratedReplay> {
    let max_lap = data
        .laps
        .iter()
        .map(|lap| lap.lap.lap_number)
        .max()
        .unwrap_or(session.total_laps);
    session.total_laps = max_lap.max(session.total_laps);

    let geometry_source = match data.source {
        crate::normalization::RaceDataSource::FastF1Historical => {
            crate::domain::TrackGeometrySource::FastF1Telemetry
        }
        crate::normalization::RaceDataSource::OpenF1Historical
        | crate::normalization::RaceDataSource::Demo => {
            crate::domain::TrackGeometrySource::OpenF1Location
        }
    };
    let geometry_locations = if data.geometry_locations.is_empty() {
        data.locations.as_slice()
    } else {
        data.geometry_locations.as_slice()
    };
    let track_geometry = super::track_geometry_builder::build_track_geometry(
        session.session_key,
        geometry_locations,
        geometry_source,
    );
    let max_t = max_time(&data).unwrap_or(DEFAULT_DURATION_SECONDS);
    let index = super::indexed_data::ReplayDataIndex::new(&data);
    let mut snapshots = Vec::new();
    let mut frame_index = 0_i64;
    while frame_index as f64 * SNAPSHOT_STEP_SECONDS <= max_t + f64::EPSILON {
        let t = frame_time(frame_index);
        snapshots.push(super::snapshot_builder::build_indexed_snapshot(
            &session,
            &index,
            &track_geometry,
            t,
            frame_index,
        ));
        frame_index += 1;
    }

    if snapshots.is_empty() {
        snapshots.push(super::snapshot_builder::build_indexed_snapshot(
            &session,
            &index,
            &track_geometry,
            0.0,
            0,
        ));
    }

    let metadata = super::metadata_builder::build_metadata(
        &session,
        &data,
        &track_geometry,
        max_t,
        SNAPSHOT_STEP_SECONDS,
        snapshots.len() as i64,
    );
    let events = super::events::generate_events(&data);

    Ok(GeneratedReplay {
        metadata,
        snapshots,
        events,
        session,
        track_geometry,
    })
}

fn frame_time(frame_index: i64) -> f64 {
    let t = frame_index as f64 * SNAPSHOT_STEP_SECONDS;
    (t * 1_000.0).round() / 1_000.0
}

fn max_time(data: &RaceData) -> Option<f64> {
    data.laps
        .iter()
        .map(|lap| lap.t_start + lap.lap.lap_duration.unwrap_or(0.0))
        .chain(data.positions.iter().map(|position| position.t))
        .chain(data.locations.iter().map(|location| location.t))
        .chain(data.weather.iter().map(|weather| weather.t))
        .chain(data.race_control.iter().map(|event| event.t))
        .max_by(f64::total_cmp)
        .filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            Driver, MapMode, Stint, TrackGeometryQuality, TrackGeometrySource, TrackPositionSample,
            TyreCompound,
        },
        normalization::{LapRecord, PositionRecord, SessionResult},
    };

    #[test]
    fn generates_snapshots_from_normalized_records() {
        let session = Session {
            session_key: 1,
            meeting_key: 1,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2024-01-01T00:00:00Z".to_string(),
            end_time: "2024-01-01T02:00:00Z".to_string(),
            total_laps: 0,
        };
        let driver = Driver {
            driver_number: 4,
            code: "NOR".to_string(),
            full_name: "Lando Norris".to_string(),
            team_name: "McLaren".to_string(),
            team_colour: "FF8000".to_string(),
        };
        let data = RaceData {
            source: crate::normalization::RaceDataSource::OpenF1Historical,
            drivers: vec![driver],
            laps: vec![LapRecord {
                t_start: 5.0,
                lap: crate::domain::Lap {
                    driver_number: 4,
                    lap_number: 1,
                    lap_duration: Some(91.0),
                    sector_1: Some(18.0),
                    sector_2: Some(34.0),
                    sector_3: Some(22.0),
                    is_pit_out_lap: false,
                },
            }],
            intervals: vec![],
            positions: vec![PositionRecord {
                t: 5.0,
                position: 1,
                rank_source: crate::domain::RankSource::OpenF1Position,
                sample: TrackPositionSample {
                    driver_number: 4,
                    x: 10.0,
                    y: 20.0,
                    z: None,
                    relative_distance: None,
                    source: crate::domain::TrackPositionSource::Schematic,
                    quality: crate::domain::TrackPositionQuality::Missing,
                    stale_seconds: None,
                },
            }],
            locations: vec![crate::normalization::LocationRecord {
                t: 5.0,
                driver_number: 4,
                x: 10.0,
                y: 20.0,
                z: None,
            }],
            geometry_locations: vec![],
            pits: vec![],
            race_control: vec![],
            stints: vec![Stint {
                driver_number: 4,
                stint_number: 1,
                compound: TyreCompound::Medium,
                lap_start: 1,
                lap_end: None,
                tyre_age_at_start: Some(0),
            }],
            weather: vec![],
            session_results: vec![SessionResult {
                driver_number: 4,
                position: Some(1),
                dnf: false,
                dns: false,
                dsq: false,
            }],
        };

        let generated = generate_replay(session, data).unwrap();
        assert_eq!(generated.metadata.session.total_laps, 1);
        assert_eq!(generated.metadata.frame_step_seconds, SNAPSHOT_STEP_SECONDS);
        assert_eq!(generated.snapshots[1].cursor.t, SNAPSHOT_STEP_SECONDS);
        assert!(generated.snapshots.len() > 1);
        assert_eq!(generated.snapshots[1].timing.rows[0].driver.code, "NOR");
    }

    #[test]
    fn bahrain_without_location_uses_projected_track_positions() {
        let session = Session {
            session_key: crate::replay::BAHRAIN_SESSION_KEY,
            meeting_key: 1229,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2024-03-02T15:00:00Z".to_string(),
            end_time: "2024-03-02T17:00:00Z".to_string(),
            total_laps: 57,
        };
        let driver = Driver {
            driver_number: 1,
            code: "VER".to_string(),
            full_name: "Max Verstappen".to_string(),
            team_name: "Red Bull Racing".to_string(),
            team_colour: "3671C6".to_string(),
        };
        let data = RaceData {
            source: crate::normalization::RaceDataSource::OpenF1Historical,
            drivers: vec![driver],
            laps: vec![LapRecord {
                t_start: 0.0,
                lap: crate::domain::Lap {
                    driver_number: 1,
                    lap_number: 1,
                    lap_duration: Some(90.0),
                    sector_1: None,
                    sector_2: None,
                    sector_3: None,
                    is_pit_out_lap: false,
                },
            }],
            intervals: vec![],
            positions: vec![PositionRecord {
                t: 0.0,
                position: 1,
                rank_source: crate::domain::RankSource::OpenF1Position,
                sample: TrackPositionSample {
                    driver_number: 1,
                    x: 0.0,
                    y: 0.0,
                    z: None,
                    relative_distance: None,
                    source: crate::domain::TrackPositionSource::Schematic,
                    quality: crate::domain::TrackPositionQuality::Missing,
                    stale_seconds: None,
                },
            }],
            locations: vec![],
            geometry_locations: vec![],
            pits: vec![],
            race_control: vec![],
            stints: vec![],
            weather: vec![],
            session_results: vec![],
        };

        let generated = generate_replay(session, data).unwrap();
        assert_eq!(
            generated.track_geometry.source,
            TrackGeometrySource::CuratedStatic
        );
        assert_eq!(
            generated.metadata.track_geometry.status,
            TrackGeometryQuality::Ready
        );
        assert_eq!(generated.snapshots[1].track.map_mode, MapMode::Projected);
        assert_eq!(
            generated.snapshots[1].track.quality,
            crate::domain::DataQuality::Projected
        );
        assert_eq!(
            generated.snapshots[1].track.positions[0].source,
            crate::domain::TrackPositionSource::Projected
        );
    }

    #[test]
    fn snapshot_json_uses_nested_v1_sections_only() {
        let session = Session {
            session_key: crate::replay::BAHRAIN_SESSION_KEY,
            meeting_key: 1229,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2024-03-02T15:00:00Z".to_string(),
            end_time: "2024-03-02T17:00:00Z".to_string(),
            total_laps: 57,
        };
        let driver = Driver {
            driver_number: 1,
            code: "VER".to_string(),
            full_name: "Max Verstappen".to_string(),
            team_name: "Red Bull Racing".to_string(),
            team_colour: "3671C6".to_string(),
        };
        let data = RaceData {
            source: crate::normalization::RaceDataSource::OpenF1Historical,
            drivers: vec![driver],
            laps: vec![],
            intervals: vec![],
            positions: vec![],
            locations: vec![],
            geometry_locations: vec![],
            pits: vec![],
            race_control: vec![],
            stints: vec![],
            weather: vec![],
            session_results: vec![],
        };

        let generated = generate_replay(session, data).unwrap();
        let payload = serde_json::to_value(&generated.snapshots[0]).unwrap();

        assert!(payload.get("cursor").is_some());
        assert!(payload.get("race_state").is_some());
        assert!(payload.get("timing").is_some());
        assert!(payload.get("track").is_some());
        assert!(payload.get("lap").is_none());
        assert!(payload.get("track_status").is_none());
        assert!(payload.get("drivers").is_none());
        assert!(payload.get("positions").is_none());
    }
}
