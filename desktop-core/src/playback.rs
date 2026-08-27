//! Port of `frontend/src/lib/replayPlayback.ts`.

use chrono::{DateTime, Utc};
use interval_backend::domain::{
    LiveAvailability, Meeting, ReplayMetadata, ReplaySnapshot, Session, TrackGeometry,
};

const ALLOWED_SPEEDS: [f64; 4] = [0.5, 1.0, 2.0, 4.0];

pub fn clamp_replay_time(t: f64, max_t: f64) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    if !max_t.is_finite() || max_t <= 0.0 {
        return t.max(0.0);
    }
    max_t.min(t.max(0.0))
}

pub fn advance_replay_time(current: f64, elapsed_seconds: f64, speed: f64, max_t: f64) -> f64 {
    if !elapsed_seconds.is_finite() || elapsed_seconds <= 0.0 {
        return clamp_replay_time(current, max_t);
    }
    clamp_replay_time(current + elapsed_seconds * normalize_replay_speed(speed), max_t)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextReplayTickOptions {
    pub current_time: f64,
    pub elapsed_seconds: f64,
    pub speed: f64,
    pub max_t: Option<f64>,
    pub playing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayTick {
    pub time: f64,
    pub playing: bool,
}

pub fn next_replay_tick(options: NextReplayTickOptions) -> ReplayTick {
    let Some(max_t) = options.max_t.filter(|_| options.playing) else {
        return ReplayTick {
            time: options.current_time,
            playing: options.playing,
        };
    };

    let time = advance_replay_time(
        options.current_time,
        options.elapsed_seconds,
        options.speed,
        max_t,
    );

    ReplayTick {
        time,
        playing: time < max_t,
    }
}

pub fn normalize_replay_speed(speed: f64) -> f64 {
    if ALLOWED_SPEEDS.contains(&speed) {
        speed
    } else {
        1.0
    }
}

pub fn parse_replay_time_input(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().filter(|parsed| parsed.is_finite())
}

pub fn parse_replay_speed_input(value: &str) -> f64 {
    normalize_replay_speed(parse_replay_time_input(value).unwrap_or(1.0))
}

pub fn quantize_replay_frame_time(t: f64, metadata: &ReplayMetadata) -> f64 {
    let clamped = clamp_replay_time(t, metadata.max_t);
    let step = metadata.frame_step_seconds;
    if !step.is_finite() || step <= 0.0 {
        return clamped;
    }

    let min_t = if metadata.min_t.is_finite() {
        metadata.min_t
    } else {
        0.0
    };
    if clamped <= min_t {
        return min_t;
    }

    let frame_index = ((clamped - min_t) / step).floor();
    clamp_replay_time(min_t + frame_index * step, metadata.max_t)
}

pub fn should_reload_session(current_session_key: Option<i64>, next_session_key: i64) -> bool {
    current_session_key == Some(next_session_key)
}

pub fn replay_resource_session_key(
    current_session_key: Option<i64>,
    metadata: Option<&ReplayMetadata>,
) -> Option<i64> {
    let current = current_session_key?;
    metadata
        .filter(|metadata| metadata.session.session_key == current)
        .map(|_| current)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapshotRequest {
    pub key: i64,
    pub t: f64,
}

pub fn snapshot_request(
    current_session_key: Option<i64>,
    metadata: Option<&ReplayMetadata>,
    t: f64,
) -> Option<SnapshotRequest> {
    let key = replay_resource_session_key(current_session_key, metadata)?;
    let metadata = metadata?;
    Some(SnapshotRequest {
        key,
        t: quantize_replay_frame_time(t, metadata),
    })
}

pub fn active_replay_metadata<'a>(
    current_session_key: Option<i64>,
    metadata: Option<&'a ReplayMetadata>,
) -> Option<&'a ReplayMetadata> {
    let current = current_session_key?;
    metadata.filter(|metadata| metadata.session.session_key == current)
}

pub fn active_replay_snapshot<'a>(
    current_session_key: Option<i64>,
    snapshot: Option<&'a ReplaySnapshot>,
) -> Option<&'a ReplaySnapshot> {
    let current = current_session_key?;
    snapshot.filter(|snapshot| snapshot.cursor.session_key == current)
}

pub fn active_track_geometry<'a>(
    current_session_key: Option<i64>,
    geometry: Option<&'a TrackGeometry>,
) -> Option<&'a TrackGeometry> {
    let current = current_session_key?;
    geometry.filter(|geometry| geometry.session_key == current)
}

pub fn active_resource_error<T>(
    current_session_key: Option<i64>,
    resource_session_key: Option<i64>,
    error: Option<T>,
) -> Option<T> {
    let current = current_session_key?;
    if resource_session_key == Some(current) {
        error
    } else {
        None
    }
}

pub fn active_resource_loading(
    current_session_key: Option<i64>,
    resource_session_key: Option<i64>,
    loading: bool,
) -> bool {
    let Some(current) = current_session_key else {
        return false;
    };
    resource_session_key == Some(current) && loading
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReplayLoadMessageOptions<'a> {
    pub metadata: Option<&'a ReplayMetadata>,
    pub metadata_loading: bool,
    pub metadata_error: Option<&'a str>,
    pub snapshot_loading: bool,
    pub snapshot_error: Option<&'a str>,
    pub session_key: Option<i64>,
    pub selected_session_label: Option<&'a str>,
    pub live_status_message: Option<&'a str>,
    pub live_connecting: bool,
}

pub fn replay_load_message(options: ReplayLoadMessageOptions) -> String {
    let selected_label = options.selected_session_label.filter(|label| !label.is_empty());
    if options.live_connecting {
        return non_empty_trimmed(options.live_status_message)
            .unwrap_or("Connecting to live session...")
            .to_string();
    }
    if options.metadata_loading && options.metadata.is_none() {
        return "Connecting to replay cache...".to_string();
    }
    if options.snapshot_loading && options.metadata.is_some() {
        return "Loading replay frame...".to_string();
    }
    if let Some(error) = options.metadata_error {
        if is_missing_replay(error) {
            return match selected_label {
                Some(label) => missing_replay_prompt(label),
                None => {
                    "Replay is not cached yet. Select a supported race or sprint to ingest it."
                        .to_string()
                }
            };
        }
        return error_text(Some(error), "Replay metadata unavailable.");
    }
    if let Some(error) = options.snapshot_error {
        return error_text(Some(error), "Replay snapshot unavailable.");
    }
    if options.metadata.is_some() {
        return "Loading replay frame...".to_string();
    }
    if options.session_key.is_none() {
        return match selected_label {
            Some(label) => missing_replay_prompt(label),
            None => non_empty_trimmed(options.live_status_message)
                .unwrap_or(
                    "Live races open automatically when available. Select a historical race or sprint to replay.",
                )
                .to_string(),
        };
    }
    "Connecting to replay cache...".to_string()
}

fn missing_replay_prompt(label: &str) -> String {
    format!("No cached replay for selected race: {label}. Ingest starts automatically when supported.")
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClearMissingHistoricalReplayOptions<'a> {
    pub metadata_error: Option<&'a str>,
    pub metadata_loading: bool,
    pub session_key: Option<i64>,
    pub live_active: bool,
    pub live_simulation_active: bool,
}

pub fn should_clear_missing_historical_replay(
    options: ClearMissingHistoricalReplayOptions,
) -> bool {
    options.session_key.is_some()
        && !options.metadata_loading
        && !options.live_active
        && !options.live_simulation_active
        && options.metadata_error.is_some_and(is_missing_replay)
}

pub fn should_apply_live_start_result(request_id: u64, latest_request_id: u64) -> bool {
    request_id == latest_request_id
}

pub fn should_apply_snapshot_result(
    request_id: u64,
    latest_request_id: u64,
    requested_session_key: i64,
    current_session_key: Option<i64>,
    live_active: bool,
    live_simulation_active: bool,
    live_transitioning: bool,
) -> bool {
    request_id == latest_request_id
        && Some(requested_session_key) == current_session_key
        && !live_active
        && !live_simulation_active
        && !live_transitioning
}

pub fn should_apply_live_resource_result(
    resource_session_key: Option<i64>,
    current_session_key: Option<i64>,
    live_active: bool,
) -> bool {
    live_active && resource_session_key.is_some() && resource_session_key == current_session_key
}

pub fn should_hide_historical_resource_error(live_active: bool) -> bool {
    live_active
}

pub fn openf1_live_session_key_to_stop(
    current_session_key: Option<i64>,
    live_active: bool,
) -> Option<i64> {
    if live_active { current_session_key } else { None }
}

pub fn live_simulation_session_key_to_stop(
    current_session_key: Option<i64>,
    live_simulation_active: bool,
) -> Option<i64> {
    if live_simulation_active {
        current_session_key
    } else {
        None
    }
}

pub fn session_key_after_live_stops(
    live_session_key: Option<i64>,
    return_session_key: Option<i64>,
) -> Option<i64> {
    return_session_key.filter(|&key| Some(key) != live_session_key)
}

pub fn replay_session_title(metadata: &ReplayMetadata) -> String {
    let session_name = session_display_name(&metadata.session);
    let meeting_name = metadata
        .meeting
        .as_ref()
        .map(|meeting| meeting.name.trim())
        .filter(|name| !name.is_empty());
    match meeting_name {
        Some(meeting) => format!("{} {} · {}", metadata.session.year, meeting, session_name),
        None => format!(
            "{} {} · #{}",
            metadata.session.year, session_name, metadata.session.session_key
        ),
    }
}

pub fn server_sent_error_message(data: Option<&str>) -> Option<String> {
    data.filter(|data| !data.trim().is_empty()).map(str::to_string)
}

pub fn live_current_message(
    availability: &LiveAvailability,
    message: Option<&str>,
    next_session: Option<&Session>,
    next_meeting: Option<&Meeting>,
) -> Option<String> {
    match availability {
        LiveAvailability::Disabled => Some(
            non_empty_trimmed(message)
                .unwrap_or("LIVE disabled")
                .to_string(),
        ),
        LiveAvailability::Inactive => {
            if let Some(next_session) = next_session {
                let meeting = next_meeting
                    .map(|meeting| meeting.name.trim())
                    .filter(|name| !name.is_empty());
                let session = session_display_name(next_session);
                let start = live_session_start_label(&next_session.start_time);
                return Some(match meeting {
                    Some(meeting) => format!(
                        "Next live: {} {} · {}{}",
                        next_session.year, meeting, session, start
                    ),
                    None => format!("Next live: {} {}{}", next_session.year, session, start),
                });
            }
            Some(
                non_empty_trimmed(message)
                    .unwrap_or("No active live race or sprint")
                    .to_string(),
            )
        }
        LiveAvailability::Error => Some(
            non_empty_trimmed(message)
                .unwrap_or("OpenF1 live status unavailable")
                .to_string(),
        ),
        LiveAvailability::Active => None,
    }
}

pub fn live_check_error_message(error: Option<&str>) -> String {
    error_text(error, "OpenF1 live status unavailable.")
}

pub fn live_start_error_message(error: Option<&str>) -> String {
    let text = error_text(error, "OpenF1 live session could not be opened.");
    if is_waiting_for_openf1_live_data_error(error) {
        format!("Waiting for OpenF1 live data. {text}")
    } else {
        text
    }
}

pub fn is_waiting_for_openf1_live_data_error(error: Option<&str>) -> bool {
    error_text(error, "").contains("OpenF1 live initial snapshot has no")
}

pub fn live_availability_after_start_error(error: Option<&str>) -> LiveAvailability {
    if is_waiting_for_openf1_live_data_error(error) {
        LiveAvailability::Inactive
    } else {
        LiveAvailability::Error
    }
}

pub fn should_poll_live_availability(availability: &LiveAvailability) -> bool {
    matches!(
        availability,
        LiveAvailability::Inactive | LiveAvailability::Error
    )
}

pub fn live_availability_poll_delay_ms(
    availability: &LiveAvailability,
    message: Option<&str>,
) -> f64 {
    if *availability == LiveAvailability::Inactive
        && message.is_some_and(|message| message.starts_with("Waiting for OpenF1 live data."))
    {
        return 10_000.0;
    }
    if matches!(
        availability,
        LiveAvailability::Inactive | LiveAvailability::Error
    ) {
        return 60_000.0;
    }
    f64::INFINITY
}

fn session_display_name(session: &Session) -> String {
    let trimmed = session.name.trim();
    if trimmed.is_empty() {
        format!("Session {}", session.session_key)
    } else {
        trimmed.to_string()
    }
}

fn is_missing_replay(error: &str) -> bool {
    let text = error.to_lowercase();
    text.contains("resource not found") || text.contains("404")
}

fn error_text(error: Option<&str>, fallback: &str) -> String {
    match error {
        Some(text) if !text.trim().is_empty() => text.to_string(),
        _ => fallback.to_string(),
    }
}

fn non_empty_trimmed(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

fn live_session_start_label(start_time: &str) -> String {
    match DateTime::parse_from_rfc3339(start_time) {
        Ok(parsed) => format!(
            " · {} UTC",
            parsed.with_timezone(&Utc).format("%Y-%m-%d %H:%M")
        ),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::{
        AvailableChannels, EndpointLinks, MapMode, RaceControlSection, RaceState, ReplayCursor,
        ReplayWeatherSection, SessionType, TimingSection, TrackBounds, TrackGeometryQuality,
        TrackGeometrySource, TrackGeometrySummary, TrackSection, REPLAY_CONTRACT_VERSION,
    };

    fn session(session_key: i64) -> Session {
        Session {
            session_key,
            meeting_key: 1229,
            year: 2024,
            name: "Race".to_string(),
            session_type: SessionType::Race,
            start_time: String::new(),
            end_time: String::new(),
            total_laps: 57,
        }
    }

    fn meeting() -> Meeting {
        Meeting {
            meeting_key: 1229,
            year: 2024,
            name: "Bahrain Grand Prix".to_string(),
            country: "Bahrain".to_string(),
            location: "Sakhir".to_string(),
        }
    }

    fn metadata(session_key: i64) -> ReplayMetadata {
        metadata_with(session_key, 1.0, 0.0)
    }

    fn metadata_with(session_key: i64, frame_step_seconds: f64, max_t: f64) -> ReplayMetadata {
        ReplayMetadata {
            contract_version: REPLAY_CONTRACT_VERSION.to_string(),
            session: session(session_key),
            meeting: Some(meeting()),
            duration_seconds: 0.0,
            frame_step_seconds,
            total_frames: 0,
            drivers: vec![],
            min_t: 0.0,
            max_t,
            race_start_t: 0.0,
            generated_at: String::new(),
            data_sources: vec![],
            available_channels: AvailableChannels {
                timing: true,
                location: false,
                track_geometry: true,
                weather: false,
                race_control: false,
                stints: false,
                pit_events: false,
                intervals: false,
            },
            track_geometry: TrackGeometrySummary {
                status: TrackGeometryQuality::Ready,
                source: TrackGeometrySource::CuratedStatic,
                quality: TrackGeometryQuality::Ready,
            },
            endpoints: EndpointLinks::default(),
        }
    }

    fn snapshot(session_key: i64) -> ReplaySnapshot {
        ReplaySnapshot {
            contract_version: REPLAY_CONTRACT_VERSION.to_string(),
            cursor: ReplayCursor {
                session_key,
                t: 0.0,
                frame_index: 0,
                playback_speed: 1.0,
                is_paused: true,
            },
            race_state: RaceState {
                lap: 1,
                track_status: "green".to_string(),
            },
            timing: TimingSection::default(),
            track: TrackSection::default(),
            weather: ReplayWeatherSection::default(),
            race_control: RaceControlSection::default(),
            derived_metrics: vec![],
        }
    }

    fn geometry(session_key: i64) -> TrackGeometry {
        TrackGeometry {
            contract_version: REPLAY_CONTRACT_VERSION.to_string(),
            session_key,
            bounds: TrackBounds {
                min_x: 0.0,
                max_x: 1.0,
                min_y: 0.0,
                max_y: 1.0,
            },
            centerline: vec![],
            inner_edge: vec![],
            outer_edge: vec![],
            source: TrackGeometrySource::Schematic,
            quality: TrackGeometryQuality::Schematic,
            map_mode: MapMode::Schematic,
            circuit_length: None,
            generated_at: String::new(),
        }
    }

    #[test]
    fn keeps_replay_time_inside_the_available_range() {
        assert_eq!(clamp_replay_time(-5.0, 100.0), 0.0);
        assert_eq!(clamp_replay_time(105.0, 100.0), 100.0);
        assert_eq!(clamp_replay_time(25.0, 100.0), 25.0);
    }

    #[test]
    fn handles_invalid_numeric_input_without_leaking_nan() {
        assert_eq!(clamp_replay_time(f64::NAN, 100.0), 0.0);
        assert_eq!(clamp_replay_time(12.0, f64::NAN), 12.0);
    }

    #[test]
    fn advances_by_elapsed_time_and_normalized_playback_speed() {
        assert_eq!(advance_replay_time(10.0, 2.0, 4.0, 100.0), 18.0);
        assert_eq!(advance_replay_time(98.0, 2.0, 4.0, 100.0), 100.0);
    }

    #[test]
    fn ignores_invalid_elapsed_values() {
        assert_eq!(advance_replay_time(10.0, -1.0, 2.0, 100.0), 10.0);
        assert_eq!(advance_replay_time(10.0, f64::NAN, 2.0, 100.0), 10.0);
    }

    #[test]
    fn accepts_supported_speeds_and_falls_back_for_unsupported_values() {
        assert_eq!(normalize_replay_speed(0.5), 0.5);
        assert_eq!(normalize_replay_speed(4.0), 4.0);
        assert_eq!(normalize_replay_speed(3.0), 1.0);
        assert_eq!(normalize_replay_speed(f64::NAN), 1.0);
    }

    #[test]
    fn parses_numeric_seek_input_and_ignores_empty_or_invalid_values() {
        assert_eq!(parse_replay_time_input("42.5"), Some(42.5));
        assert_eq!(parse_replay_time_input(""), None);
        assert_eq!(parse_replay_time_input("   "), None);
        assert_eq!(parse_replay_time_input("not-a-time"), None);
    }

    #[test]
    fn normalizes_speed_input_from_the_control() {
        assert_eq!(parse_replay_speed_input("2"), 2.0);
        assert_eq!(parse_replay_speed_input("3"), 1.0);
        assert_eq!(parse_replay_speed_input(""), 1.0);
    }

    #[test]
    fn uses_the_replay_frame_cadence_to_request_persisted_frames() {
        let replay_metadata = metadata_with(9472, 0.5, 102.0);

        assert_eq!(quantize_replay_frame_time(0.0, &replay_metadata), 0.0);
        assert_eq!(quantize_replay_frame_time(0.49, &replay_metadata), 0.0);
        assert_eq!(quantize_replay_frame_time(0.5, &replay_metadata), 0.5);
        assert_eq!(quantize_replay_frame_time(101.9, &replay_metadata), 101.5);
    }

    #[test]
    fn falls_back_to_clamping_when_metadata_has_no_usable_frame_cadence() {
        let replay_metadata = metadata_with(9472, 0.0, 100.0);

        assert_eq!(quantize_replay_frame_time(12.5, &replay_metadata), 12.5);
        assert_eq!(quantize_replay_frame_time(150.0, &replay_metadata), 100.0);
    }

    #[test]
    fn keeps_idle_or_unloaded_replay_state_unchanged() {
        assert_eq!(
            next_replay_tick(NextReplayTickOptions {
                current_time: 10.0,
                elapsed_seconds: 5.0,
                speed: 2.0,
                playing: false,
                max_t: Some(100.0),
            }),
            ReplayTick {
                time: 10.0,
                playing: false,
            }
        );

        assert_eq!(
            next_replay_tick(NextReplayTickOptions {
                current_time: 10.0,
                elapsed_seconds: 5.0,
                speed: 2.0,
                playing: true,
                max_t: None,
            }),
            ReplayTick {
                time: 10.0,
                playing: true,
            }
        );
    }

    #[test]
    fn advances_playing_replay_state_and_stops_at_the_end() {
        assert_eq!(
            next_replay_tick(NextReplayTickOptions {
                current_time: 10.0,
                elapsed_seconds: 5.0,
                speed: 2.0,
                playing: true,
                max_t: Some(100.0),
            }),
            ReplayTick {
                time: 20.0,
                playing: true,
            }
        );

        assert_eq!(
            next_replay_tick(NextReplayTickOptions {
                current_time: 98.0,
                elapsed_seconds: 5.0,
                speed: 1.0,
                playing: true,
                max_t: Some(100.0),
            }),
            ReplayTick {
                time: 100.0,
                playing: false,
            }
        );
    }

    #[test]
    fn distinguishes_same_session_reload_from_session_switch() {
        assert!(should_reload_session(Some(9472), 9472));
        assert!(!should_reload_session(Some(9839), 9472));
        assert!(!should_reload_session(None, 9472));
    }

    #[test]
    fn uses_metadata_only_when_it_belongs_to_the_active_session() {
        assert_eq!(
            replay_resource_session_key(Some(9472), Some(&metadata(9472))),
            Some(9472)
        );
        assert_eq!(
            replay_resource_session_key(Some(9839), Some(&metadata(9472))),
            None
        );
        assert_eq!(replay_resource_session_key(Some(9472), None), None);
        assert_eq!(replay_resource_session_key(None, Some(&metadata(9472))), None);
    }

    #[test]
    fn builds_snapshot_requests_only_from_active_session_metadata() {
        assert_eq!(
            snapshot_request(Some(9472), Some(&metadata_with(9472, 0.5, 100.0)), 27.4),
            Some(SnapshotRequest { key: 9472, t: 27.0 })
        );
        assert_eq!(snapshot_request(Some(9839), Some(&metadata(9472)), 25.0), None);
    }

    #[test]
    fn filters_stale_resource_values_by_active_session() {
        assert_eq!(
            active_replay_metadata(Some(9472), Some(&metadata(9472)))
                .map(|metadata| metadata.session.session_key),
            Some(9472)
        );
        assert!(active_replay_metadata(Some(9839), Some(&metadata(9472))).is_none());
        assert!(active_replay_metadata(None, Some(&metadata(9472))).is_none());
        assert_eq!(
            active_replay_snapshot(Some(9472), Some(&snapshot(9472)))
                .map(|snapshot| snapshot.cursor.session_key),
            Some(9472)
        );
        assert!(active_replay_snapshot(Some(9839), Some(&snapshot(9472))).is_none());
        assert!(active_replay_snapshot(None, Some(&snapshot(9472))).is_none());
        assert_eq!(
            active_track_geometry(Some(9472), Some(&geometry(9472)))
                .map(|geometry| geometry.session_key),
            Some(9472)
        );
        assert!(active_track_geometry(Some(9839), Some(&geometry(9472))).is_none());
        assert!(active_track_geometry(None, Some(&geometry(9472))).is_none());
    }

    #[test]
    fn filters_stale_resource_errors_by_active_session() {
        let error = "geometry unavailable";

        assert_eq!(
            active_resource_error(Some(9472), Some(9472), Some(error)),
            Some(error)
        );
        assert_eq!(active_resource_error(Some(9839), Some(9472), Some(error)), None);
        assert_eq!(active_resource_error(Some(9472), None, Some(error)), None);
        assert_eq!(active_resource_error(None, Some(9472), Some(error)), None);
        assert_eq!(active_resource_error::<&str>(Some(9472), Some(9472), None), None);
    }

    #[test]
    fn filters_stale_resource_loading_states_by_active_session() {
        assert!(active_resource_loading(Some(9472), Some(9472), true));
        assert!(!active_resource_loading(Some(9472), Some(9472), false));
        assert!(!active_resource_loading(Some(9839), Some(9472), true));
        assert!(!active_resource_loading(Some(9472), None, true));
        assert!(!active_resource_loading(None, Some(9472), true));
    }

    #[test]
    fn uses_specific_errors_when_available() {
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata_error: Some("metadata missing"),
                ..Default::default()
            }),
            "metadata missing"
        );
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                snapshot_error: Some("snapshot missing"),
                ..Default::default()
            }),
            "snapshot missing"
        );
    }

    #[test]
    fn keeps_stale_errors_hidden_while_the_next_replay_resource_is_loading() {
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata_loading: true,
                metadata_error: Some("previous metadata error"),
                ..Default::default()
            }),
            "Connecting to replay cache..."
        );

        let replay_metadata = metadata(9472);
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata: Some(&replay_metadata),
                snapshot_loading: true,
                snapshot_error: Some("previous snapshot error"),
                ..Default::default()
            }),
            "Loading replay frame..."
        );
    }

    #[test]
    fn turns_missing_historical_metadata_into_an_ingest_prompt() {
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata_error: Some("resource not found"),
                session_key: Some(9472),
                ..Default::default()
            }),
            "Replay is not cached yet. Select a supported race or sprint to ingest it."
        );
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata_error: Some("404 Not Found"),
                session_key: Some(42),
                selected_session_label: Some("2025 Race #42"),
                ..Default::default()
            }),
            "No cached replay for selected race: 2025 Race #42. Ingest starts automatically when supported."
        );
    }

    #[test]
    fn explains_empty_selection_and_selected_uncached_states() {
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                session_key: None,
                ..Default::default()
            }),
            "Live races open automatically when available. Select a historical race or sprint to replay."
        );
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                session_key: None,
                live_status_message: Some("Checking live race status..."),
                ..Default::default()
            }),
            "Checking live race status..."
        );
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                session_key: None,
                selected_session_label: Some("2025 Race #1234"),
                live_status_message: Some("No active live race or sprint"),
                ..Default::default()
            }),
            "No cached replay for selected race: 2025 Race #1234. Ingest starts automatically when supported."
        );
    }

    #[test]
    fn distinguishes_metadata_and_frame_loading_states() {
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                session_key: Some(9472),
                ..Default::default()
            }),
            "Connecting to replay cache..."
        );
        let replay_metadata = metadata(9472);
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata: Some(&replay_metadata),
                ..Default::default()
            }),
            "Loading replay frame..."
        );
    }

    #[test]
    fn describes_a_live_connection_instead_of_a_historical_frame_load() {
        let replay_metadata = metadata(9472);
        assert_eq!(
            replay_load_message(ReplayLoadMessageOptions {
                metadata: Some(&replay_metadata),
                snapshot_loading: true,
                live_connecting: true,
                live_status_message: Some("Opening active OpenF1 live session..."),
                ..Default::default()
            }),
            "Opening active OpenF1 live session..."
        );
    }

    #[test]
    fn clears_stale_historical_sessions_when_cached_metadata_is_missing() {
        assert!(should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("resource not found"),
                session_key: Some(9472),
                ..Default::default()
            }
        ));
        assert!(should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("404 Not Found"),
                session_key: Some(9472),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn does_not_clear_active_live_sessions_when_historical_metadata_is_missing() {
        assert!(!should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("resource not found"),
                session_key: Some(88001),
                live_active: true,
                ..Default::default()
            }
        ));
        assert!(!should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("404 Not Found"),
                session_key: Some(9839),
                live_simulation_active: true,
                ..Default::default()
            }
        ));
    }

    #[test]
    fn ignores_loading_non_missing_errors_and_empty_session_state() {
        assert!(!should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("resource not found"),
                metadata_loading: true,
                session_key: Some(9472),
                ..Default::default()
            }
        ));
        assert!(!should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("database unavailable"),
                session_key: Some(9472),
                ..Default::default()
            }
        ));
        assert!(!should_clear_missing_historical_replay(
            ClearMissingHistoricalReplayOptions {
                metadata_error: Some("resource not found"),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn only_applies_the_latest_live_start_request() {
        assert!(should_apply_live_start_result(3, 3));
        assert!(!should_apply_live_start_result(2, 3));
    }

    #[test]
    fn rejects_stale_or_live_mode_historical_snapshot_results() {
        assert!(should_apply_snapshot_result(3, 3, 9472, Some(9472), false, false, false));
        assert!(!should_apply_snapshot_result(2, 3, 9472, Some(9472), false, false, false));
        assert!(!should_apply_snapshot_result(3, 3, 9472, Some(9839), false, false, false));
        assert!(!should_apply_snapshot_result(3, 3, 9472, Some(9472), true, false, false));
        assert!(!should_apply_snapshot_result(3, 3, 9472, Some(9472), false, true, false));
        assert!(!should_apply_snapshot_result(3, 3, 9472, Some(9472), false, false, true));
    }

    #[test]
    fn applies_live_resources_only_for_the_active_live_session() {
        assert!(should_apply_live_resource_result(Some(88001), Some(88001), true));
        assert!(!should_apply_live_resource_result(Some(88001), Some(9472), true));
        assert!(!should_apply_live_resource_result(Some(88001), Some(88001), false));
        assert!(!should_apply_live_resource_result(None, Some(88001), true));
    }

    #[test]
    fn hides_historical_cache_errors_while_real_live_mode_owns_resources() {
        assert!(should_hide_historical_resource_error(true));
        assert!(!should_hide_historical_resource_error(false));
    }

    #[test]
    fn returns_a_previous_replay_session_only_when_it_differs_from_the_live_session() {
        assert_eq!(session_key_after_live_stops(Some(88001), Some(9472)), Some(9472));
        assert_eq!(session_key_after_live_stops(Some(88001), Some(88001)), None);
        assert_eq!(session_key_after_live_stops(Some(88001), None), None);
    }

    #[test]
    fn returns_the_active_live_key_when_leaving_openf1_live_mode() {
        assert_eq!(openf1_live_session_key_to_stop(Some(88001), true), Some(88001));
        assert_eq!(openf1_live_session_key_to_stop(Some(88001), false), None);
        assert_eq!(openf1_live_session_key_to_stop(None, true), None);
    }

    #[test]
    fn returns_the_active_simulation_key_when_leaving_live_simulation_mode() {
        assert_eq!(live_simulation_session_key_to_stop(Some(9472), true), Some(9472));
        assert_eq!(live_simulation_session_key_to_stop(Some(9472), false), None);
        assert_eq!(live_simulation_session_key_to_stop(None, true), None);
    }

    #[test]
    fn uses_meeting_context_when_metadata_includes_it() {
        assert_eq!(
            replay_session_title(&metadata(9472)),
            "2024 Bahrain Grand Prix · Race"
        );
    }

    #[test]
    fn falls_back_to_session_identity_when_meeting_context_is_unavailable() {
        let mut replay_metadata = metadata(9472);
        replay_metadata.meeting = None;
        assert_eq!(replay_session_title(&replay_metadata), "2024 Race · #9472");
    }

    #[test]
    fn extracts_backend_sse_error_payloads() {
        assert_eq!(
            server_sent_error_message(Some("live snapshot refresh failed")),
            Some("live snapshot refresh failed".to_string())
        );
    }

    #[test]
    fn ignores_transport_style_error_events_without_payload_data() {
        assert_eq!(server_sent_error_message(None), None);
        assert_eq!(server_sent_error_message(Some("")), None);
    }

    #[test]
    fn uses_backend_provided_live_availability_messages_when_present() {
        assert_eq!(
            live_current_message(
                &LiveAvailability::Error,
                Some("OpenF1 live configuration error"),
                None,
                None
            ),
            Some("OpenF1 live configuration error".to_string())
        );
        assert_eq!(
            live_current_message(&LiveAvailability::Inactive, Some("No race today"), None, None),
            Some("No race today".to_string())
        );
    }

    #[test]
    fn labels_the_next_live_candidate_when_inactive() {
        let mut next_session = session(88001);
        next_session.start_time = "2024-03-02T15:00:00Z".to_string();
        assert_eq!(
            live_current_message(
                &LiveAvailability::Inactive,
                None,
                Some(&next_session),
                Some(&meeting())
            ),
            Some("Next live: 2024 Bahrain Grand Prix · Race · 2024-03-02 15:00 UTC".to_string())
        );
    }

    #[test]
    fn omits_the_next_live_start_time_when_the_timestamp_is_invalid() {
        let mut next_session = session(88001);
        next_session.start_time = "not-a-date".to_string();
        assert_eq!(
            live_current_message(
                &LiveAvailability::Inactive,
                None,
                Some(&next_session),
                Some(&meeting())
            ),
            Some("Next live: 2024 Bahrain Grand Prix · Race".to_string())
        );
    }

    #[test]
    fn falls_back_to_concise_live_availability_labels() {
        assert_eq!(
            live_current_message(&LiveAvailability::Disabled, None, None, None),
            Some("LIVE disabled".to_string())
        );
        assert_eq!(
            live_current_message(&LiveAvailability::Inactive, None, None, None),
            Some("No active live race or sprint".to_string())
        );
        assert_eq!(
            live_current_message(&LiveAvailability::Error, None, None, None),
            Some("OpenF1 live status unavailable".to_string())
        );
        assert_eq!(live_current_message(&LiveAvailability::Active, None, None, None), None);
    }

    #[test]
    fn preserves_thrown_live_check_errors_for_actionable_feedback() {
        assert_eq!(
            live_check_error_message(Some("OpenF1 live configuration error")),
            "OpenF1 live configuration error"
        );
        assert_eq!(
            live_check_error_message(Some("network unavailable")),
            "network unavailable"
        );
    }

    #[test]
    fn preserves_live_start_backend_errors_for_actionable_feedback() {
        assert_eq!(
            live_start_error_message(Some("OpenF1 live initial snapshot has no driver data")),
            "Waiting for OpenF1 live data. OpenF1 live initial snapshot has no driver data"
        );
        assert_eq!(
            live_start_error_message(None),
            "OpenF1 live session could not be opened."
        );
    }

    #[test]
    fn identifies_temporary_openf1_live_warmup_errors() {
        assert!(is_waiting_for_openf1_live_data_error(Some(
            "OpenF1 live initial snapshot has no timing/location data"
        )));
        assert!(!is_waiting_for_openf1_live_data_error(Some(
            "OpenF1 live request failed"
        )));
    }

    #[test]
    fn keeps_temporary_live_warmup_failures_in_the_retryable_availability_state() {
        assert_eq!(
            live_availability_after_start_error(Some(
                "OpenF1 live initial snapshot has no driver data"
            )),
            LiveAvailability::Inactive
        );
        assert_eq!(
            live_availability_after_start_error(Some("OpenF1 live request failed")),
            LiveAvailability::Error
        );
    }

    #[test]
    fn retries_inactive_and_transient_error_states_only() {
        assert!(should_poll_live_availability(&LiveAvailability::Inactive));
        assert!(should_poll_live_availability(&LiveAvailability::Error));
        assert!(!should_poll_live_availability(&LiveAvailability::Disabled));
        assert!(!should_poll_live_availability(&LiveAvailability::Active));
    }

    #[test]
    fn keeps_warmup_checks_responsive_but_backs_off_ordinary_inactive_and_error_states() {
        assert_eq!(
            live_availability_poll_delay_ms(
                &LiveAvailability::Inactive,
                Some("Waiting for OpenF1 live data. OpenF1 live initial snapshot has no driver data")
            ),
            10_000.0
        );
        assert_eq!(
            live_availability_poll_delay_ms(
                &LiveAvailability::Inactive,
                Some("No active live race or sprint")
            ),
            60_000.0
        );
        assert_eq!(
            live_availability_poll_delay_ms(
                &LiveAvailability::Error,
                Some("OpenF1 live status unavailable")
            ),
            60_000.0
        );
    }

    #[test]
    fn does_not_schedule_polling_for_terminal_availability_states() {
        assert_eq!(
            live_availability_poll_delay_ms(&LiveAvailability::Disabled, None),
            f64::INFINITY
        );
        assert_eq!(
            live_availability_poll_delay_ms(&LiveAvailability::Active, None),
            f64::INFINITY
        );
    }
}
