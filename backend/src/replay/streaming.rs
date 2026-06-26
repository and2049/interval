use crate::domain::{ReplayEvent, ReplayMetadata};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayFrameWindow {
    pub t: f64,
    pub previous_t: f64,
}

pub fn frame_window(metadata: &ReplayMetadata, frame_index: i64) -> ReplayFrameWindow {
    let frame_step = metadata.frame_step_seconds.max(1.0);
    let t = metadata.min_t + frame_index as f64 * frame_step;
    let previous_t = if frame_index == 0 {
        f64::NEG_INFINITY
    } else {
        metadata.min_t + (frame_index - 1) as f64 * frame_step
    };

    ReplayFrameWindow { t, previous_t }
}

pub fn events_for_window<'a>(
    events: &'a [ReplayEvent],
    window: ReplayFrameWindow,
) -> impl Iterator<Item = &'a ReplayEvent> {
    events
        .iter()
        .filter(move |event| event.t > window.previous_t && event.t <= window.t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AvailableChannels, EndpointLinks, EventKind, EventSeverity, EventSource, Session,
        SessionType, TrackGeometrySummary,
    };

    #[test]
    fn frame_window_uses_metadata_cadence() {
        let metadata = metadata(5.0);

        assert_eq!(
            frame_window(&metadata, 3),
            ReplayFrameWindow {
                t: 15.0,
                previous_t: 10.0
            }
        );
    }

    #[test]
    fn first_frame_window_includes_pre_session_events() {
        let metadata = metadata(5.0);

        assert_eq!(frame_window(&metadata, 0).previous_t, f64::NEG_INFINITY);
    }

    #[test]
    fn events_for_window_includes_each_event_once() {
        let events = vec![
            event("before", -5.0),
            event("first", 0.0),
            event("previous", 5.0),
            event("inside", 9.9),
            event("next", 10.1),
        ];

        let selected = events_for_window(
            &events,
            ReplayFrameWindow {
                previous_t: 5.0,
                t: 10.0,
            },
        )
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>();

        assert_eq!(selected, vec!["inside"]);
    }

    fn event(id: &str, t: f64) -> ReplayEvent {
        ReplayEvent {
            id: id.to_string(),
            t,
            kind: EventKind::RaceControl,
            severity: EventSeverity::Info,
            driver_number: None,
            message: id.to_string(),
            source: EventSource::System,
            payload: serde_json::Value::Null,
        }
    }

    fn metadata(frame_step_seconds: f64) -> ReplayMetadata {
        ReplayMetadata {
            contract_version: crate::domain::REPLAY_CONTRACT_VERSION.to_string(),
            session: Session {
                session_key: 1,
                meeting_key: 1,
                year: 2024,
                name: "Race".to_string(),
                session_type: SessionType::Race,
                start_time: String::new(),
                end_time: String::new(),
                total_laps: 1,
            },
            meeting: None,
            duration_seconds: 100.0,
            frame_step_seconds,
            total_frames: 20,
            drivers: vec![],
            min_t: 0.0,
            max_t: 100.0,
            generated_at: String::new(),
            data_sources: vec![],
            available_channels: AvailableChannels {
                timing: true,
                location: false,
                track_geometry: false,
                weather: false,
                race_control: false,
                stints: false,
                pit_events: false,
                intervals: false,
            },
            track_geometry: TrackGeometrySummary::default(),
            endpoints: EndpointLinks {
                snapshot_endpoint: String::new(),
                stream_endpoint: String::new(),
                events_endpoint: String::new(),
                track_geometry_endpoint: String::new(),
            },
        }
    }
}
