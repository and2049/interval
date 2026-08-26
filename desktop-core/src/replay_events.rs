//! Port of `frontend/src/lib/replayEvents.ts`.

use crate::formatters::Tone;
use interval_backend::domain::{EventKind, EventSeverity, ReplayEvent};

pub const DEFAULT_EVENT_LIMIT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventFeedState {
    Ready,
    Loading,
    Error,
    Empty,
}

pub fn recent_replay_events(events: &[ReplayEvent], t: f64, limit: usize) -> Vec<ReplayEvent> {
    let cursor = if t.is_finite() { t } else { 0.0 };
    let mut recent: Vec<ReplayEvent> = events
        .iter()
        .filter(|event| event.t <= cursor)
        .cloned()
        .collect();
    recent.sort_by(|a, b| b.t.total_cmp(&a.t));
    recent.truncate(limit);
    recent
}

pub fn event_kind_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::RaceControl => "RACE CONTROL",
        EventKind::TrackStatus => "TRACK STATUS",
        EventKind::PitStop => "PIT STOP",
        EventKind::StintChange => "STINT CHANGE",
        EventKind::LeaderChange => "LEADER CHANGE",
        EventKind::WeatherChange => "WEATHER CHANGE",
        EventKind::DataGap => "DATA GAP",
        EventKind::DriverOut => "DRIVER OUT",
    }
}

pub fn event_severity_class(severity: &EventSeverity) -> Tone {
    match severity {
        EventSeverity::Critical => Tone::Danger,
        EventSeverity::Warning => Tone::Amber,
        EventSeverity::Notice => Tone::Mint,
        EventSeverity::Info => Tone::Neutral,
    }
}

pub fn event_feed_state(rows: &[ReplayEvent], loading: bool, error: bool) -> EventFeedState {
    if !rows.is_empty() {
        return EventFeedState::Ready;
    }
    if loading {
        return EventFeedState::Loading;
    }
    if error {
        return EventFeedState::Error;
    }
    EventFeedState::Empty
}

pub fn event_feed_empty_label(state: EventFeedState) -> &'static str {
    match state {
        EventFeedState::Loading => "Loading replay events...",
        EventFeedState::Error => "Replay event feed unavailable",
        EventFeedState::Empty => "No replay events yet",
        EventFeedState::Ready => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::EventSource;

    fn event(id: &str, t: f64) -> ReplayEvent {
        ReplayEvent {
            id: id.to_string(),
            t,
            kind: EventKind::RaceControl,
            severity: EventSeverity::Info,
            driver_number: None,
            message: id.to_string(),
            source: EventSource::OpenF1,
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn returns_latest_events_at_or_before_the_replay_cursor() {
        assert_eq!(
            recent_replay_events(
                &[event("a", 10.0), event("b", 20.0), event("c", 30.0)],
                25.0,
                DEFAULT_EVENT_LIMIT,
            ),
            vec![event("b", 20.0), event("a", 10.0)]
        );
    }

    #[test]
    fn limits_output_and_handles_invalid_cursors() {
        assert_eq!(
            recent_replay_events(
                &[event("a", 0.0), event("b", 1.0), event("c", 2.0)],
                3.0,
                2,
            ),
            vec![event("c", 2.0), event("b", 1.0)]
        );
        assert_eq!(
            recent_replay_events(&[event("a", 1.0)], f64::NAN, DEFAULT_EVENT_LIMIT),
            vec![]
        );
    }

    #[test]
    fn formats_event_kind_labels_and_severity_classes() {
        assert_eq!(event_kind_label(&EventKind::LeaderChange), "LEADER CHANGE");
        assert_eq!(event_severity_class(&EventSeverity::Critical), Tone::Danger);
        assert_eq!(event_severity_class(&EventSeverity::Warning), Tone::Amber);
        assert_eq!(event_severity_class(&EventSeverity::Notice), Tone::Mint);
        assert_eq!(event_severity_class(&EventSeverity::Info), Tone::Neutral);
    }

    #[test]
    fn labels_loading_error_empty_and_ready_feed_states() {
        assert_eq!(
            event_feed_state(&[event("a", 1.0)], true, false),
            EventFeedState::Ready
        );
        assert_eq!(event_feed_state(&[], true, false), EventFeedState::Loading);
        assert_eq!(event_feed_state(&[], false, true), EventFeedState::Error);
        assert_eq!(event_feed_state(&[], false, false), EventFeedState::Empty);

        assert_eq!(
            event_feed_empty_label(EventFeedState::Loading),
            "Loading replay events..."
        );
        assert_eq!(
            event_feed_empty_label(EventFeedState::Error),
            "Replay event feed unavailable"
        );
        assert_eq!(
            event_feed_empty_label(EventFeedState::Empty),
            "No replay events yet"
        );
        assert_eq!(event_feed_empty_label(EventFeedState::Ready), "");
    }
}
