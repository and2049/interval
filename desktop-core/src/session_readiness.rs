//! Port of `frontend/src/lib/sessionReadiness.ts`: the ingest/readiness state
//! machine labels — action button label/enabled state, status badge content per
//! `SessionReadiness`, and ingest outcome messages.

use interval_backend::domain::{IngestResponse, IngestStatus, SessionReadiness, SessionSupportStatus};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionActionState {
    #[default]
    Idle,
    Checking,
    OpeningCache,
    Ingesting,
    OpeningReplay,
    Failed,
}

/// Semantic color tone; the GPUI layer maps these to theme colors
/// (mint/amber/danger accents, neutral border + muted text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Mint,
    Amber,
    Danger,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcomeTone {
    Ready,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestOutcome {
    pub label: String,
    pub tone: IngestOutcomeTone,
    pub title: Option<String>,
}

pub fn ingest_status_text(status: &IngestStatus) -> &'static str {
    match status {
        IngestStatus::NotIngested => "not ingested",
        IngestStatus::Fetching => "fetching",
        IngestStatus::Normalizing => "normalizing",
        IngestStatus::Ready => "ready",
        IngestStatus::Failed => "failed",
    }
}

pub fn session_status_label(entry: &SessionReadiness) -> String {
    if entry.support_status == SessionSupportStatus::Cancelled {
        return "cancelled".to_string();
    }
    if entry.support_status == SessionSupportStatus::Future {
        return "not available yet".to_string();
    }
    if entry.is_demo {
        return "demo".to_string();
    }
    if entry.replay_ready {
        return "ready".to_string();
    }
    ingest_status_text(&entry.ingest_status).to_string()
}

pub fn session_status_class(status: &IngestStatus) -> Tone {
    match status {
        IngestStatus::Ready => Tone::Mint,
        IngestStatus::Failed => Tone::Danger,
        IngestStatus::Fetching | IngestStatus::Normalizing => Tone::Amber,
        IngestStatus::NotIngested => Tone::Neutral,
    }
}

pub fn session_status_badge_text(entry: &SessionReadiness) -> String {
    if entry.support_status == SessionSupportStatus::Cancelled {
        return "CANCELLED".to_string();
    }
    if entry.support_status == SessionSupportStatus::Future {
        return "FUTURE".to_string();
    }
    if entry.is_demo {
        return "DEMO".to_string();
    }
    ingest_status_text(&entry.ingest_status).to_uppercase()
}

pub fn session_action_label(
    ingest_state: SessionActionState,
    selected_session: Option<i64>,
    active_session_key: Option<i64>,
    readiness: Option<&SessionReadiness>,
) -> &'static str {
    if readiness.is_some_and(|entry| !is_session_supported(Some(entry))) {
        return "UNAVAILABLE";
    }
    match ingest_state {
        SessionActionState::Checking => return "CHECKING",
        SessionActionState::OpeningCache | SessionActionState::OpeningReplay => return "OPENING",
        SessionActionState::Ingesting => return "INGESTING",
        SessionActionState::Failed => return "RETRY",
        SessionActionState::Idle => {}
    }
    let Some(selected_session) = selected_session else {
        return "SELECT SESSION";
    };
    if readiness.is_some_and(|entry| entry.replay_ready || entry.is_demo) {
        return if Some(selected_session) == active_session_key {
            "RELOAD"
        } else {
            "OPEN CACHE"
        };
    }
    "INGEST + OPEN"
}

pub fn is_session_supported(readiness: Option<&SessionReadiness>) -> bool {
    readiness.is_none_or(|entry| entry.support_status == SessionSupportStatus::Supported)
}

pub fn can_start_session_action(readiness: Option<&SessionReadiness>) -> bool {
    readiness.is_some() && is_session_supported(readiness)
}

pub fn is_session_action_disabled(
    selected_session: Option<i64>,
    readiness: Option<&SessionReadiness>,
    ingest_state: SessionActionState,
    live_active: bool,
) -> bool {
    selected_session.is_none()
        || live_active
        || !can_start_session_action(readiness)
        || is_busy_session_action(ingest_state)
}

pub fn can_open_session_from_cache(readiness: Option<&SessionReadiness>) -> bool {
    is_session_supported(readiness)
        && readiness.is_some_and(|entry| entry.replay_ready || entry.is_demo)
}

pub fn can_open_session_after_ingest(response: &IngestResponse) -> bool {
    response.status == IngestStatus::Ready && response.generated_snapshots > 0
}

pub fn should_clear_transient_session_action(
    previous_session: Option<i64>,
    selected_session: Option<i64>,
    ingest_state: SessionActionState,
) -> bool {
    previous_session != selected_session
        && previous_session.is_some()
        && !is_busy_session_action(ingest_state)
}

pub fn is_busy_session_action(state: SessionActionState) -> bool {
    matches!(
        state,
        SessionActionState::Checking
            | SessionActionState::OpeningCache
            | SessionActionState::Ingesting
            | SessionActionState::OpeningReplay
    )
}

pub fn session_action_status(state: SessionActionState) -> Option<&'static str> {
    match state {
        SessionActionState::Checking => Some("Checking selected replay..."),
        SessionActionState::OpeningCache => Some("Opening cached replay..."),
        SessionActionState::Ingesting => Some("Ingesting selected session..."),
        SessionActionState::OpeningReplay => Some("Opening replay..."),
        SessionActionState::Failed | SessionActionState::Idle => None,
    }
}

pub fn session_ingest_error_message(
    ingest_error: Option<&str>,
    readiness: Option<&SessionReadiness>,
) -> String {
    ingest_error
        .map(str::to_string)
        .or_else(|| readiness.and_then(|entry| entry.support_reason.clone()))
        .or_else(|| readiness.and_then(|entry| entry.last_error.clone()))
        .unwrap_or_else(|| "Ingest failed.".to_string())
}

pub fn ingest_outcome(response: Option<&IngestResponse>) -> Option<IngestOutcome> {
    let response = response?;
    if response.status == IngestStatus::Failed {
        return Some(IngestOutcome {
            label: response
                .error
                .clone()
                .unwrap_or_else(|| "Ingest failed.".to_string()),
            tone: IngestOutcomeTone::Failed,
            title: None,
        });
    }

    let frame_label = if response.generated_snapshots == 1 {
        "1 frame".to_string()
    } else {
        format!("{} frames", format_thousands(response.generated_snapshots))
    };
    if response.warnings.is_empty() {
        return Some(IngestOutcome {
            label: format!("Cached {frame_label}"),
            tone: IngestOutcomeTone::Ready,
            title: None,
        });
    }

    let warning_label = if response.warnings.len() == 1 {
        "1 warning".to_string()
    } else {
        format!("{} warnings", response.warnings.len())
    };
    Some(IngestOutcome {
        label: format!("Cached {frame_label} · {warning_label}"),
        tone: IngestOutcomeTone::Degraded,
        title: Some(response.warnings.join(" | ")),
    })
}

pub fn ingest_outcome_class(tone: IngestOutcomeTone) -> Tone {
    match tone {
        IngestOutcomeTone::Ready => Tone::Mint,
        IngestOutcomeTone::Degraded => Tone::Amber,
        IngestOutcomeTone::Failed => Tone::Danger,
    }
}

// Mirrors the TS `Number.prototype.toLocaleString()` (en-US) frame counts.
fn format_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::{Session, SessionType};

    fn readiness() -> SessionReadiness {
        SessionReadiness {
            session: Session {
                session_key: 9472,
                meeting_key: 1229,
                year: 2024,
                name: "Race".to_string(),
                session_type: SessionType::Race,
                start_time: String::new(),
                end_time: String::new(),
                total_laps: 57,
            },
            ingest_status: IngestStatus::NotIngested,
            replay_ready: false,
            is_demo: false,
            last_error: None,
            support_status: SessionSupportStatus::Supported,
            support_reason: None,
        }
    }

    fn ingest_response(status: IngestStatus, generated_snapshots: usize) -> IngestResponse {
        let failed = status == IngestStatus::Failed;
        IngestResponse {
            session_key: 9472,
            status,
            cached_endpoints: 11,
            endpoint_coverage: vec![],
            generated_snapshots,
            track_geometry: None,
            available_channels: None,
            warnings: vec![],
            error: failed.then(|| "OpenF1 unavailable".to_string()),
        }
    }

    #[test]
    fn labels_demo_and_ready_sessions_before_raw_ingest_status() {
        let mut demo = readiness();
        demo.is_demo = true;
        demo.replay_ready = true;
        assert_eq!(session_status_label(&demo), "demo");

        let mut ready = readiness();
        ready.replay_ready = true;
        assert_eq!(session_status_label(&ready), "ready");
    }

    #[test]
    fn humanizes_non_ready_ingest_status() {
        assert_eq!(session_status_label(&readiness()), "not ingested");
    }

    #[test]
    fn labels_unsupported_sessions_before_ingest_status() {
        let mut cancelled = readiness();
        cancelled.support_status = SessionSupportStatus::Cancelled;
        assert_eq!(session_status_label(&cancelled), "cancelled");

        let mut future = readiness();
        future.support_status = SessionSupportStatus::Future;
        assert_eq!(session_status_label(&future), "not available yet");
    }

    #[test]
    fn uses_semantic_tones_for_ingest_states() {
        assert_eq!(session_status_class(&IngestStatus::Ready), Tone::Mint);
        assert_eq!(session_status_class(&IngestStatus::Failed), Tone::Danger);
        assert_eq!(session_status_class(&IngestStatus::Fetching), Tone::Amber);
        assert_eq!(session_status_class(&IngestStatus::NotIngested), Tone::Neutral);
    }

    #[test]
    fn formats_demo_and_ingest_statuses_for_compact_badges() {
        let mut demo = readiness();
        demo.is_demo = true;
        assert_eq!(session_status_badge_text(&demo), "DEMO");

        assert_eq!(session_status_badge_text(&readiness()), "NOT INGESTED");

        let mut cancelled = readiness();
        cancelled.support_status = SessionSupportStatus::Cancelled;
        assert_eq!(session_status_badge_text(&cancelled), "CANCELLED");
    }

    #[test]
    fn labels_missing_and_in_progress_selections() {
        assert_eq!(
            session_action_label(SessionActionState::Idle, None, Some(9472), None),
            "SELECT SESSION"
        );
        assert_eq!(
            session_action_label(SessionActionState::Ingesting, Some(9472), Some(9472), None),
            "INGESTING"
        );
    }

    #[test]
    fn distinguishes_cached_open_cached_reload_and_ingest() {
        let mut ready = readiness();
        ready.replay_ready = true;
        assert_eq!(
            session_action_label(SessionActionState::Idle, Some(9472), Some(9839), Some(&ready)),
            "OPEN CACHE"
        );
        assert_eq!(
            session_action_label(SessionActionState::Idle, Some(9472), Some(9472), Some(&ready)),
            "RELOAD"
        );

        let not_ready = readiness();
        assert_eq!(
            session_action_label(
                SessionActionState::Idle,
                Some(9472),
                Some(9839),
                Some(&not_ready)
            ),
            "INGEST + OPEN"
        );

        let mut cancelled = readiness();
        cancelled.support_status = SessionSupportStatus::Cancelled;
        assert_eq!(
            session_action_label(SessionActionState::Idle, Some(9472), None, Some(&cancelled)),
            "UNAVAILABLE"
        );
    }

    #[test]
    fn labels_automatic_selection_progress_and_retry_states() {
        assert_eq!(
            session_action_label(SessionActionState::Checking, Some(9472), None, None),
            "CHECKING"
        );
        assert_eq!(
            session_action_label(SessionActionState::OpeningCache, Some(9472), None, None),
            "OPENING"
        );
        assert_eq!(
            session_action_label(SessionActionState::OpeningReplay, Some(9472), None, None),
            "OPENING"
        );
        assert_eq!(
            session_action_label(SessionActionState::Failed, Some(9472), None, None),
            "RETRY"
        );
    }

    #[test]
    fn identify_busy_auto_open_states() {
        assert!(is_busy_session_action(SessionActionState::Checking));
        assert!(is_busy_session_action(SessionActionState::OpeningCache));
        assert!(is_busy_session_action(SessionActionState::Ingesting));
        assert!(is_busy_session_action(SessionActionState::OpeningReplay));
        assert!(!is_busy_session_action(SessionActionState::Idle));
        assert!(!is_busy_session_action(SessionActionState::Failed));
    }

    #[test]
    fn formats_compact_progress_messages() {
        assert_eq!(
            session_action_status(SessionActionState::Checking),
            Some("Checking selected replay...")
        );
        assert_eq!(
            session_action_status(SessionActionState::OpeningCache),
            Some("Opening cached replay...")
        );
        assert_eq!(
            session_action_status(SessionActionState::Ingesting),
            Some("Ingesting selected session...")
        );
        assert_eq!(
            session_action_status(SessionActionState::OpeningReplay),
            Some("Opening replay...")
        );
        assert_eq!(session_action_status(SessionActionState::Idle), None);
        assert_eq!(session_action_status(SessionActionState::Failed), None);
    }

    #[test]
    fn locks_historical_open_and_ingest_actions_while_live_owns_the_dashboard() {
        let mut ready = readiness();
        ready.replay_ready = true;

        assert!(!is_session_action_disabled(
            Some(9472),
            Some(&ready),
            SessionActionState::Idle,
            false
        ));
        assert!(is_session_action_disabled(
            Some(9472),
            Some(&ready),
            SessionActionState::Idle,
            true
        ));
    }

    #[test]
    fn disables_actions_for_missing_unavailable_or_busy_selections() {
        let mut ready = readiness();
        ready.replay_ready = true;
        assert!(is_session_action_disabled(
            None,
            Some(&ready),
            SessionActionState::Idle,
            false
        ));

        let mut cancelled = readiness();
        cancelled.support_status = SessionSupportStatus::Cancelled;
        assert!(is_session_action_disabled(
            Some(9472),
            Some(&cancelled),
            SessionActionState::Idle,
            false
        ));

        assert!(is_session_action_disabled(
            Some(9472),
            Some(&ready),
            SessionActionState::Ingesting,
            false
        ));
    }

    #[test]
    fn opens_ready_or_demo_sessions_without_ingesting() {
        let mut ready = readiness();
        ready.replay_ready = true;
        assert!(can_open_session_from_cache(Some(&ready)));

        let mut demo = readiness();
        demo.is_demo = true;
        assert!(can_open_session_from_cache(Some(&demo)));

        assert!(!can_open_session_from_cache(Some(&readiness())));

        let mut cancelled_ready = readiness();
        cancelled_ready.replay_ready = true;
        cancelled_ready.support_status = SessionSupportStatus::Cancelled;
        assert!(!can_open_session_from_cache(Some(&cancelled_ready)));

        assert!(!can_open_session_from_cache(None));
    }

    #[test]
    fn opens_only_ready_ingest_responses_with_generated_replay_frames() {
        assert!(can_open_session_after_ingest(&ingest_response(
            IngestStatus::Ready,
            1200
        )));
        assert!(!can_open_session_after_ingest(&ingest_response(
            IngestStatus::Ready,
            0
        )));
        assert!(!can_open_session_after_ingest(&ingest_response(
            IngestStatus::Failed,
            0
        )));
    }

    #[test]
    fn clears_stale_ingest_feedback_when_the_selected_session_changes() {
        assert!(should_clear_transient_session_action(
            Some(9472),
            Some(9839),
            SessionActionState::Failed
        ));
    }

    #[test]
    fn keeps_initial_selection_and_in_progress_ingest_state_stable() {
        assert!(!should_clear_transient_session_action(
            None,
            Some(9472),
            SessionActionState::Failed
        ));
        assert!(!should_clear_transient_session_action(
            Some(9472),
            Some(9839),
            SessionActionState::Ingesting
        ));
    }

    #[test]
    fn chooses_explicit_errors_before_readiness_errors_and_fallback_text() {
        assert_eq!(
            session_ingest_error_message(Some("fetch failed"), None),
            "fetch failed"
        );

        let mut future = readiness();
        future.support_status = SessionSupportStatus::Future;
        future.support_reason = Some("Not run yet".to_string());
        assert_eq!(session_ingest_error_message(None, Some(&future)), "Not run yet");

        let mut errored = readiness();
        errored.last_error = Some("cached failure".to_string());
        assert_eq!(
            session_ingest_error_message(None, Some(&errored)),
            "cached failure"
        );

        assert_eq!(session_ingest_error_message(None, None), "Ingest failed.");
    }

    #[test]
    fn summarizes_successful_ingest_output() {
        assert_eq!(
            ingest_outcome(Some(&ingest_response(IngestStatus::Ready, 1200))),
            Some(IngestOutcome {
                label: "Cached 1,200 frames".to_string(),
                tone: IngestOutcomeTone::Ready,
                title: None,
            })
        );
    }

    #[test]
    fn summarizes_degraded_ingest_output_with_warning_details() {
        let mut response = ingest_response(IngestStatus::Ready, 1200);
        response.warnings = vec![
            "location missing".to_string(),
            "weather missing".to_string(),
        ];
        assert_eq!(
            ingest_outcome(Some(&response)),
            Some(IngestOutcome {
                label: "Cached 1,200 frames · 2 warnings".to_string(),
                tone: IngestOutcomeTone::Degraded,
                title: Some("location missing | weather missing".to_string()),
            })
        );
    }

    #[test]
    fn summarizes_failed_ingest_output() {
        assert_eq!(
            ingest_outcome(Some(&ingest_response(IngestStatus::Failed, 0))),
            Some(IngestOutcome {
                label: "OpenF1 unavailable".to_string(),
                tone: IngestOutcomeTone::Failed,
                title: None,
            })
        );
        assert_eq!(ingest_outcome(None), None);
    }

    #[test]
    fn maps_ingest_outcome_tones_to_compact_text_classes() {
        assert_eq!(ingest_outcome_class(IngestOutcomeTone::Ready), Tone::Mint);
        assert_eq!(ingest_outcome_class(IngestOutcomeTone::Degraded), Tone::Amber);
        assert_eq!(ingest_outcome_class(IngestOutcomeTone::Failed), Tone::Danger);
    }
}
