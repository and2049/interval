use crate::{
    domain::{
        AvailableChannels, DataSource, EndpointLinks, ReplayMetadata, Session, TrackGeometry,
        TrackGeometryQuality, TrackGeometrySummary, REPLAY_CONTRACT_VERSION,
    },
    normalization::RaceData,
};
use chrono::Utc;

pub fn build_metadata(
    session: &Session,
    data: &RaceData,
    geometry: &TrackGeometry,
    max_t: f64,
    frame_step_seconds: f64,
    total_frames: i64,
) -> ReplayMetadata {
    ReplayMetadata {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session: session.clone(),
        meeting: None,
        duration_seconds: max_t,
        frame_step_seconds,
        total_frames,
        drivers: data.drivers.clone(),
        min_t: 0.0,
        max_t,
        generated_at: Utc::now().to_rfc3339(),
        data_sources: vec![DataSource {
            name: "openf1".to_string(),
            mode: "historical".to_string(),
        }],
        available_channels: available_channels(data, geometry),
        track_geometry: TrackGeometrySummary {
            status: geometry.quality.clone(),
            source: geometry.source.clone(),
            quality: geometry.quality.clone(),
        },
        endpoints: endpoint_links(session.session_key),
    }
}

fn available_channels(data: &RaceData, geometry: &TrackGeometry) -> AvailableChannels {
    AvailableChannels {
        timing: !data.drivers.is_empty(),
        location: !data.locations.is_empty(),
        track_geometry: geometry.quality == TrackGeometryQuality::Ready,
        weather: !data.weather.is_empty(),
        race_control: !data.race_control.is_empty(),
        stints: !data.stints.is_empty(),
        pit_events: !data.pits.is_empty(),
        intervals: !data.intervals.is_empty(),
    }
}

fn endpoint_links(session_key: i64) -> EndpointLinks {
    EndpointLinks {
        snapshot_endpoint: format!("/api/sessions/{session_key}/replay/snapshot?t={{t}}"),
        stream_endpoint: format!("/api/sessions/{session_key}/replay/stream"),
        events_endpoint: format!("/api/sessions/{session_key}/replay/events"),
        track_geometry_endpoint: format!("/api/sessions/{session_key}/track/geometry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::TrackGeometrySource, replay::curated_geometry};

    #[test]
    fn metadata_labels_curated_geometry_as_available_without_location_channel() {
        let session = Session {
            session_key: crate::replay::BAHRAIN_SESSION_KEY,
            meeting_key: 1229,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: String::new(),
            end_time: String::new(),
            total_laps: 57,
        };
        let data = RaceData {
            drivers: vec![crate::domain::Driver {
                driver_number: 1,
                code: "VER".to_string(),
                full_name: "Max Verstappen".to_string(),
                team_name: "Red Bull Racing".to_string(),
                team_colour: "3671C6".to_string(),
            }],
            laps: vec![],
            intervals: vec![],
            positions: vec![],
            locations: vec![],
            pits: vec![],
            race_control: vec![],
            stints: vec![],
            weather: vec![],
            session_results: vec![],
        };
        let geometry = curated_geometry(crate::replay::BAHRAIN_SESSION_KEY).unwrap();

        let metadata = build_metadata(&session, &data, &geometry, 100.0, 5.0, 20);

        assert!(metadata.available_channels.timing);
        assert!(!metadata.available_channels.location);
        assert!(metadata.available_channels.track_geometry);
        assert_eq!(
            metadata.track_geometry.source,
            TrackGeometrySource::CuratedStatic
        );
        assert!(metadata
            .endpoints
            .stream_endpoint
            .ends_with("/replay/stream"));
    }
}
