//! Port of `frontend/src/lib/replayEvents.ts`.

use crate::formatters::Tone;
use interval_backend::domain::{EventKind, EventSeverity, RaceControlMessage, ReplayEvent};

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

/// Human label for a normalized track status: the FastF1 codes the export script
/// resolves (1 clear, 2 yellow, 4 safety car, 5 red, 6 VSC, 7 VSC ending) plus the
/// states the backend derives from OpenF1's live steward messages.
pub fn flag_label(flag: &str) -> Option<&'static str> {
    match flag {
        "green" => Some("TRACK CLEAR"),
        "yellow" => Some("YELLOW FLAG"),
        "double_yellow" => Some("DOUBLE YELLOW"),
        "red" => Some("RED FLAG"),
        "safety_car" => Some("SAFETY CAR"),
        "safety_car_ending" => Some("SAFETY CAR IN THIS LAP"),
        "virtual_safety_car" => Some("VIRTUAL SAFETY CAR"),
        "virtual_safety_car_ending" => Some("VSC ENDING"),
        "chequered" => Some("CHEQUERED FLAG"),
        _ => None,
    }
}

pub fn flag_tone(flag: &str) -> Tone {
    match flag {
        "green" => Tone::Emerald,
        "yellow"
        | "double_yellow"
        | "safety_car"
        | "safety_car_ending"
        | "virtual_safety_car"
        | "virtual_safety_car_ending" => Tone::Amber,
        "red" => Tone::Danger,
        _ => Tone::Bright,
    }
}

/// The map's status chip: the readable label for a known status, else the raw value.
pub fn track_status_chip(status: &str) -> (String, Tone) {
    let label = flag_label(status)
        .map(str::to_string)
        .unwrap_or_else(|| status.replace('_', " ").to_ascii_uppercase());
    (label, flag_tone(status))
}

/// Display line for a race-control entry. Track-status rows carry a machine message
/// ("Track status 2"), so those render as the flag condition itself; real steward
/// messages pass through verbatim, toned by their flag when one is attached.
pub fn race_control_display(message: &RaceControlMessage) -> (String, Tone) {
    match message.flag.as_deref() {
        Some(flag) => {
            let label = match flag_label(flag) {
                Some(label) if message.category == "track_status" => label.to_string(),
                _ => message.message.clone(),
            };
            (label, flag_tone(flag))
        }
        None => (message.message.clone(), Tone::Bright),
    }
}

/// Feed message with the same track-status translation: `TrackStatus` events carry the
/// bare flag value, `RaceControl` events echo the source row (whose category/flag ride
/// along in the payload); everything else shows its message untouched.
pub fn event_message_label(event: &ReplayEvent) -> String {
    match event.kind {
        EventKind::TrackStatus => flag_label(&event.message)
            .map(str::to_string)
            .unwrap_or_else(|| event.message.clone()),
        EventKind::RaceControl => {
            let payload_str = |key: &str| {
                event
                    .payload
                    .get(key)
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            };
            match (payload_str("category"), payload_str("flag")) {
                (Some(category), Some(flag)) if category == "track_status" => flag_label(&flag)
                    .map(str::to_string)
                    .unwrap_or_else(|| event.message.clone()),
                _ => event.message.clone(),
            }
        }
        _ => event.message.clone(),
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

    fn rc(category: &str, message: &str, flag: Option<&str>) -> RaceControlMessage {
        RaceControlMessage {
            t: 10.0,
            category: category.to_string(),
            message: message.to_string(),
            flag: flag.map(str::to_string),
            scope: None,
        }
    }

    #[test]
    fn track_status_chip_labels_known_states_and_falls_back_to_the_raw_value() {
        assert_eq!(
            track_status_chip("virtual_safety_car"),
            ("VIRTUAL SAFETY CAR".to_string(), Tone::Amber)
        );
        assert_eq!(track_status_chip("red"), ("RED FLAG".to_string(), Tone::Danger));
        assert_eq!(track_status_chip("green"), ("TRACK CLEAR".to_string(), Tone::Emerald));
        assert_eq!(track_status_chip("double_yellow"), ("DOUBLE YELLOW".to_string(), Tone::Amber));
        assert_eq!(track_status_chip("some_new_state"), ("SOME NEW STATE".to_string(), Tone::Bright));
    }

    #[test]
    fn track_status_rows_render_as_flag_conditions() {
        assert_eq!(
            race_control_display(&rc("track_status", "Track status 1", Some("green"))),
            ("TRACK CLEAR".to_string(), Tone::Emerald)
        );
        assert_eq!(
            race_control_display(&rc("track_status", "Track status 2", Some("yellow"))),
            ("YELLOW FLAG".to_string(), Tone::Amber)
        );
        assert_eq!(
            race_control_display(&rc("track_status", "Track status 5", Some("red"))),
            ("RED FLAG".to_string(), Tone::Danger)
        );
        // Unknown raw code: no flag mapping, so the raw message stays visible.
        assert_eq!(
            race_control_display(&rc("track_status", "Track status 9", Some("9"))),
            ("Track status 9".to_string(), Tone::Bright)
        );
    }

    #[test]
    fn steward_messages_pass_through_toned_by_their_flag() {
        assert_eq!(
            race_control_display(&rc("Flag", "YELLOW IN TRACK SECTOR 7", Some("yellow"))),
            ("YELLOW IN TRACK SECTOR 7".to_string(), Tone::Amber)
        );
        assert_eq!(
            race_control_display(&rc("Other", "CAR 4 (NOR) TIME DELETED", None)),
            ("CAR 4 (NOR) TIME DELETED".to_string(), Tone::Bright)
        );
    }

    #[test]
    fn feed_messages_translate_track_status_via_kind_and_payload() {
        let mut track_status = event("green", 10.0);
        track_status.kind = EventKind::TrackStatus;
        assert_eq!(event_message_label(&track_status), "TRACK CLEAR");

        let mut race_control = event("Track status 2", 10.0);
        race_control.payload =
            serde_json::to_value(rc("track_status", "Track status 2", Some("yellow"))).unwrap();
        assert_eq!(event_message_label(&race_control), "YELLOW FLAG");

        // A steward message keeps its text even though a flag rides in the payload.
        let mut steward = event("YELLOW IN TRACK SECTOR 7", 10.0);
        steward.payload =
            serde_json::to_value(rc("Flag", "YELLOW IN TRACK SECTOR 7", Some("yellow"))).unwrap();
        assert_eq!(event_message_label(&steward), "YELLOW IN TRACK SECTOR 7");
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
