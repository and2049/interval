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

