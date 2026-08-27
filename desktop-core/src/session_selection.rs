//! Port of `frontend/src/lib/sessionSelection.ts`: builds the
//! Season/Meeting/Session selector option lists and the selection fallback
//! logic.

use crate::session_readiness::session_status_label;
use interval_backend::domain::{Meeting, Season, Session, SessionReadiness, SessionSupportStatus, SessionType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSelectOption {
    pub value: i64,
    pub label: String,
    pub disabled: bool,
    pub title: Option<String>,
}

/// The Season/Meeting/Session keys currently picked in the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectedSessionKeys {
    pub season: Option<i32>,
    pub meeting: Option<i64>,
    pub session: Option<i64>,
}

pub fn next_selection<T, K: PartialEq + Copy>(
    options: Option<&[T]>,
    selected: Option<K>,
    key_for: impl Fn(&T) -> K,
    preferred: Option<K>,
) -> Option<K> {
    let Some(options) = options else {
        return selected;
    };
    let first = options.first()?;
    if let Some(selected) = selected {
        if options.iter().any(|item| key_for(item) == selected) {
            return Some(selected);
        }
    }
    if let Some(preferred) = preferred {
        if options.iter().any(|item| key_for(item) == preferred) {
            return Some(preferred);
        }
    }
    Some(key_for(first))
}

pub fn next_season_selection(
    seasons: Option<&[Season]>,
    selected: Option<i32>,
    preferred: Option<i32>,
) -> Option<i32> {
    next_selection(seasons, selected, |season| season.year, preferred)
}

pub fn next_meeting_selection(
    meetings: Option<&[Meeting]>,
    selected: Option<i64>,
    preferred: Option<i64>,
) -> Option<i64> {
    next_selection(meetings, selected, |meeting| meeting.meeting_key, preferred)
}

pub fn next_session_selection(
    sessions: Option<&[SessionReadiness]>,
    selected: Option<i64>,
    preferred: Option<i64>,
) -> Option<i64> {
    let selectable = sessions.map(|entries| {
        entries
            .iter()
            .filter(|entry| entry.support_status == SessionSupportStatus::Supported)
            .collect::<Vec<_>>()
    });
    next_selection(
        selectable.as_deref(),
        selected,
        |entry| entry.session.session_key,
        preferred,
    )
}

pub fn readiness_for_session<'a>(
    sessions: Option<&'a [SessionReadiness]>,
    selected: Option<i64>,
) -> Option<&'a SessionReadiness> {
    let selected = selected?;
    sessions?
        .iter()
        .find(|entry| entry.session.session_key == selected)
}

pub fn active_session_selection_matches(active: &Session, selected: &SelectedSessionKeys) -> bool {
    selected.season == Some(active.year)
        && selected.meeting == Some(active.meeting_key)
        && selected.session == Some(active.session_key)
}

pub fn should_sync_active_session_selection(
    active: &Session,
    selected: &SelectedSessionKeys,
    last_active_session_key: Option<i64>,
) -> bool {
    if last_active_session_key != Some(active.session_key) {
        return true;
    }
    selected.session == Some(active.session_key)
        && !active_session_selection_matches(active, selected)
}

pub fn season_options(seasons: Option<&[Season]>) -> Vec<SessionSelectOption> {
    seasons
        .unwrap_or_default()
        .iter()
        .map(|season| SessionSelectOption {
            value: i64::from(season.year),
            label: season.year.to_string(),
            disabled: false,
            title: None,
        })
        .collect()
}

pub fn meeting_options(meetings: Option<&[Meeting]>) -> Vec<SessionSelectOption> {
    meetings
        .unwrap_or_default()
        .iter()
        .map(|meeting| SessionSelectOption {
            value: meeting.meeting_key,
            label: meeting.name.clone(),
            disabled: false,
            title: None,
        })
        .collect()
}

pub fn session_options(sessions: Option<&[SessionReadiness]>) -> Vec<SessionSelectOption> {
    sessions
        .unwrap_or_default()
        .iter()
        .map(|entry| SessionSelectOption {
            value: entry.session.session_key,
            label: format!(
                "{} · {}",
                session_display_name(&entry.session),
                session_status_label(entry)
            ),
            disabled: entry.support_status != SessionSupportStatus::Supported,
            title: entry.support_reason.clone(),
        })
        .collect()
}

pub fn selected_session_label(entry: Option<&SessionReadiness>) -> Option<String> {
    let entry = entry?;
    Some(format!(
        "{} {} #{}",
        entry.session.year,
        session_display_name(&entry.session),
        entry.session.session_key
    ))
}

pub fn session_type_label(session: &Session) -> &'static str {
    match session.session_type {
        SessionType::Sprint => "SPRINT",
        SessionType::Race => "RACE",
    }
}

pub fn session_display_name(session: &Session) -> String {
    let type_label = session_type_label(session);
    let type_name = match session.session_type {
        SessionType::Sprint => "sprint",
        SessionType::Race => "race",
    };
    if session.name.trim().to_lowercase() == type_name {
        type_label.to_string()
    } else {
        format!("{type_label} {}", session.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::IngestStatus;

    fn meeting(meeting_key: i64) -> Meeting {
        Meeting {
            meeting_key,
            year: 2024,
            name: format!("Meeting {meeting_key}"),
            country: String::new(),
            location: String::new(),
        }
    }

    fn readiness(session_key: i64) -> SessionReadiness {
        SessionReadiness {
            session: Session {
                session_key,
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

    fn seasons(years: &[i32]) -> Vec<Season> {
        years.iter().map(|&year| Season { year }).collect()
    }

    #[test]
    fn keeps_a_current_selection_that_still_exists() {
        assert_eq!(
            next_selection(Some(&[1, 2][..]), Some(2), |item| *item, None),
            Some(2)
        );
    }

    #[test]
    fn falls_back_to_the_first_option_when_selection_is_missing_or_stale() {
        assert_eq!(
            next_selection(Some(&[1, 2][..]), None, |item| *item, None),
            Some(1)
        );
        assert_eq!(
            next_selection(Some(&[1, 2][..]), Some(9), |item| *item, None),
            Some(1)
        );
    }

    #[test]
    fn uses_a_preferred_option_before_first_option_fallback() {
        assert_eq!(
            next_selection(Some(&[1, 2][..]), None, |item| *item, Some(2)),
            Some(2)
        );
        assert_eq!(
            next_selection(Some(&[1, 2][..]), Some(9), |item| *item, Some(2)),
            Some(2)
        );
    }

    #[test]
    fn keeps_a_valid_current_selection_ahead_of_the_preferred_option() {
        assert_eq!(
            next_selection(Some(&[1, 2][..]), Some(1), |item| *item, Some(2)),
            Some(1)
        );
    }

    #[test]
    fn preserves_selection_while_options_are_unloaded_and_clears_loaded_empty_sets() {
        assert_eq!(next_selection(Some(&[][..]), Some(2), |item: &i32| *item, None), None);
        assert_eq!(next_selection(None, Some(2), |item: &i32| *item, None), Some(2));
    }

    #[test]
    fn select_seasons_meetings_and_sessions_by_their_stable_keys() {
        assert_eq!(
            next_season_selection(Some(&seasons(&[2025, 2024])), Some(2024), None),
            Some(2024)
        );
        assert_eq!(
            next_meeting_selection(Some(&[meeting(1229), meeting(1230)]), Some(999), None),
            Some(1229)
        );
        assert_eq!(
            next_session_selection(Some(&[readiness(9472), readiness(9839)]), None, None),
            Some(9472)
        );
    }

    #[test]
    fn skips_unavailable_sessions_for_automatic_session_selection() {
        let mut cancelled = readiness(100);
        cancelled.support_status = SessionSupportStatus::Cancelled;
        assert_eq!(
            next_session_selection(Some(&[cancelled, readiness(101)]), None, None),
            Some(101)
        );

        let mut future = readiness(100);
        future.support_status = SessionSupportStatus::Future;
        assert_eq!(next_session_selection(Some(&[future]), None, None), None);
    }

    #[test]
    fn prefer_the_curated_mvp_keys_when_no_current_selection_is_active() {
        assert_eq!(
            next_season_selection(Some(&seasons(&[2026, 2024])), None, Some(2024)),
            Some(2024)
        );
        assert_eq!(
            next_meeting_selection(Some(&[meeting(1), meeting(1229)]), None, Some(1229)),
            Some(1229)
        );
        assert_eq!(
            next_session_selection(Some(&[readiness(9839), readiness(9472)]), None, Some(9472)),
            Some(9472)
        );
    }

    #[test]
    fn finds_the_selected_readiness_entry() {
        let entries = [readiness(9472), readiness(9839)];
        assert_eq!(
            readiness_for_session(Some(&entries), Some(9839)).map(|e| e.session.session_key),
            Some(9839)
        );
        assert!(readiness_for_session(Some(&entries[..1]), None).is_none());
    }

    #[test]
    fn formats_selected_session_context_for_empty_replay_messages() {
        assert_eq!(
            selected_session_label(Some(&readiness(9472))),
            Some("2024 RACE #9472".to_string())
        );

        let mut sprint = readiness(9473);
        sprint.session.session_type = SessionType::Sprint;
        sprint.session.name = "Sprint".to_string();
        assert_eq!(
            selected_session_label(Some(&sprint)),
            Some("2024 SPRINT #9473".to_string())
        );

        assert_eq!(selected_session_label(None), None);
    }

    #[test]
    fn requires_season_meeting_and_session_to_match_the_active_replay() {
        let active = readiness(9472).session;

        assert!(active_session_selection_matches(
            &active,
            &SelectedSessionKeys {
                season: Some(2024),
                meeting: Some(1229),
                session: Some(9472),
            }
        ));
        assert!(!active_session_selection_matches(
            &active,
            &SelectedSessionKeys {
                season: Some(2024),
                meeting: Some(1228),
                session: Some(9472),
            }
        ));
    }

    #[test]
    fn syncs_when_the_active_replay_session_changes() {
        let active = readiness(9472).session;

        assert!(should_sync_active_session_selection(
            &active,
            &SelectedSessionKeys {
                season: Some(2024),
                meeting: Some(1228),
                session: None,
            },
            None
        ));
    }

    #[test]
    fn repairs_stale_parent_selections_for_the_active_session_key() {
        let active = readiness(9472).session;

        assert!(should_sync_active_session_selection(
            &active,
            &SelectedSessionKeys {
                season: Some(2024),
                meeting: Some(1228),
                session: Some(9472),
            },
            Some(9472)
        ));
    }

    #[test]
    fn does_not_pin_the_selector_when_the_user_browses_away_from_the_active_session() {
        let active = readiness(9472).session;

        assert!(!should_sync_active_session_selection(
            &active,
            &SelectedSessionKeys {
                season: Some(2024),
                meeting: Some(1230),
                session: None,
            },
            Some(9472)
        ));
    }

    #[test]
    fn build_compact_season_and_meeting_options() {
        assert_eq!(
            season_options(Some(&seasons(&[2024, 2023]))),
            vec![
                SessionSelectOption {
                    value: 2024,
                    label: "2024".to_string(),
                    disabled: false,
                    title: None,
                },
                SessionSelectOption {
                    value: 2023,
                    label: "2023".to_string(),
                    disabled: false,
                    title: None,
                },
            ]
        );
        assert_eq!(
            meeting_options(Some(&[meeting(1229)])),
            vec![SessionSelectOption {
                value: 1229,
                label: "Meeting 1229".to_string(),
                disabled: false,
                title: None,
            }]
        );
    }

    #[test]
    fn include_readiness_state_in_session_option_labels() {
        let mut ready = readiness(9472);
        ready.replay_ready = true;
        assert_eq!(
            session_options(Some(&[ready])),
            vec![SessionSelectOption {
                value: 9472,
                label: "RACE · ready".to_string(),
                disabled: false,
                title: None,
            }]
        );

        let mut demo = readiness(9839);
        demo.is_demo = true;
        assert_eq!(
            session_options(Some(&[demo])),
            vec![SessionSelectOption {
                value: 9839,
                label: "RACE · demo".to_string(),
                disabled: false,
                title: None,
            }]
        );

        let mut sprint = readiness(9473);
        sprint.session.session_type = SessionType::Sprint;
        sprint.session.name = "Sprint".to_string();
        assert_eq!(
            session_options(Some(&[sprint])),
            vec![SessionSelectOption {
                value: 9473,
                label: "SPRINT · not ingested".to_string(),
                disabled: false,
                title: None,
            }]
        );

        let mut cancelled = readiness(9474);
        cancelled.support_status = SessionSupportStatus::Cancelled;
        cancelled.support_reason = Some("Event was cancelled".to_string());
        assert_eq!(
            session_options(Some(&[cancelled])),
            vec![SessionSelectOption {
                value: 9474,
                label: "RACE · cancelled".to_string(),
                disabled: true,
                title: Some("Event was cancelled".to_string()),
            }]
        );
    }

    #[test]
    fn keeps_custom_session_names_after_the_type_label() {
        let mut sprint_race = readiness(9473);
        sprint_race.session.session_type = SessionType::Sprint;
        sprint_race.session.name = "Sprint Race".to_string();
        assert_eq!(session_display_name(&sprint_race.session), "SPRINT Sprint Race");
    }
}
