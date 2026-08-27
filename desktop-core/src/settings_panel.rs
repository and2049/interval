//! Port of `frontend/src/lib/settingsPanel.ts`.

use crate::replay_quality::{BadgeTone, ChannelBadge};
use serde::{Deserialize, Serialize};

// The backend's `api::settings::OpenF1TokenSettings`/`OpenF1TokenProbe` are
// response-only types with private fields, so the client-side wire shape is
// mirrored here (matching `shared/types/api.ts`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenF1TokenSource {
    Settings,
    Env,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenF1TokenSettings {
    pub configured: bool,
    /// Masked fingerprint of the stored token. Never the token itself.
    pub hint: Option<String>,
    pub source: OpenF1TokenSource,
    /// True when INTERVAL_OPENF1_LIVE_TOKEN is also set.
    pub env_token_present: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenF1TokenProbeResult {
    Ok,
    Unauthorized,
    Unreachable,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenF1TokenProbe {
    pub result: OpenF1TokenProbeResult,
    pub message: String,
}

pub fn is_submittable_token(value: &str) -> bool {
    !value.trim().is_empty()
}

/// One line describing what the backend is currently using, and where it came from.
pub fn token_source_line(settings: &OpenF1TokenSettings) -> String {
    match settings.source {
        OpenF1TokenSource::Settings => {
            format!("Saved token {}", settings.hint.as_deref().unwrap_or(""))
                .trim()
                .to_string()
        }
        OpenF1TokenSource::Env => {
            "Using INTERVAL_OPENF1_LIVE_TOKEN from the environment".to_string()
        }
        OpenF1TokenSource::None => "No token configured".to_string(),
    }
}

/// Shown only when a saved token is shadowing an environment one. Without this the user
/// edits `.env`, sees nothing change, and has no way to find out why.
pub fn env_override_notice(settings: &OpenF1TokenSettings) -> Option<&'static str> {
    if settings.source != OpenF1TokenSource::Settings || !settings.env_token_present {
        return None;
    }
    Some(
        "INTERVAL_OPENF1_LIVE_TOKEN is also set. The token saved here takes precedence; clear it to use the environment value.",
    )
}

pub fn probe_badge(probe: &OpenF1TokenProbe) -> ChannelBadge {
    let (label, ready, tone) = match probe.result {
        OpenF1TokenProbeResult::Ok => ("TOKEN OK", true, BadgeTone::Ready),
        OpenF1TokenProbeResult::Unauthorized => ("REJECTED", false, BadgeTone::Missing),
        OpenF1TokenProbeResult::Unreachable => ("UNREACHABLE", false, BadgeTone::Missing),
        OpenF1TokenProbeResult::Invalid => ("INVALID", false, BadgeTone::Degraded),
    };
    ChannelBadge {
        label: label.to_string(),
        ready,
        tone,
        title: None,
    }
}

pub fn save_error_message(error: Option<&str>) -> String {
    match error {
        Some(message) if !message.trim().is_empty() => message.to_string(),
        _ => "Could not save the token.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SettingsOverrides {
        hint: Option<String>,
        source: OpenF1TokenSource,
        env_token_present: bool,
        configured: bool,
    }

    impl Default for SettingsOverrides {
        fn default() -> Self {
            Self {
                hint: Some("••••abcd".to_string()),
                source: OpenF1TokenSource::Settings,
                env_token_present: false,
                configured: true,
            }
        }
    }

    fn settings(overrides: SettingsOverrides) -> OpenF1TokenSettings {
        OpenF1TokenSettings {
            configured: overrides.configured,
            hint: overrides.hint,
            source: overrides.source,
            env_token_present: overrides.env_token_present,
            path: Some("/config/interval/settings.json".to_string()),
        }
    }

    fn probe(result: OpenF1TokenProbeResult) -> OpenF1TokenProbe {
        OpenF1TokenProbe {
            result,
            message: "message".to_string(),
        }
    }

    #[test]
    fn accepts_a_non_blank_token() {
        assert!(is_submittable_token("abc"));
    }

    #[test]
    fn rejects_blank_input() {
        assert!(!is_submittable_token(""));
        assert!(!is_submittable_token("   "));
    }

    #[test]
    fn shows_the_masked_hint_for_a_saved_token() {
        assert_eq!(
            token_source_line(&settings(SettingsOverrides {
                source: OpenF1TokenSource::Settings,
                hint: Some("••••n123".to_string()),
                ..Default::default()
            })),
            "Saved token ••••n123"
        );
    }

    #[test]
    fn names_the_environment_variable_when_that_is_what_is_in_use() {
        assert_eq!(
            token_source_line(&settings(SettingsOverrides {
                source: OpenF1TokenSource::Env,
                ..Default::default()
            })),
            "Using INTERVAL_OPENF1_LIVE_TOKEN from the environment"
        );
    }

    #[test]
    fn reports_when_nothing_is_configured() {
        assert_eq!(
            token_source_line(&settings(SettingsOverrides {
                source: OpenF1TokenSource::None,
                configured: false,
                ..Default::default()
            })),
            "No token configured"
        );
    }

    #[test]
    fn warns_only_when_a_saved_token_is_shadowing_an_environment_one() {
        assert!(env_override_notice(&settings(SettingsOverrides {
            source: OpenF1TokenSource::Settings,
            env_token_present: true,
            ..Default::default()
        }))
        .unwrap()
        .contains("takes precedence"));
    }

    #[test]
    fn stays_silent_when_there_is_nothing_being_shadowed() {
        assert_eq!(
            env_override_notice(&settings(SettingsOverrides {
                source: OpenF1TokenSource::Settings,
                env_token_present: false,
                ..Default::default()
            })),
            None
        );
        assert_eq!(
            env_override_notice(&settings(SettingsOverrides {
                source: OpenF1TokenSource::Env,
                env_token_present: true,
                ..Default::default()
            })),
            None
        );
        assert_eq!(
            env_override_notice(&settings(SettingsOverrides {
                source: OpenF1TokenSource::None,
                env_token_present: false,
                ..Default::default()
            })),
            None
        );
    }

    #[test]
    fn maps_each_probe_result_to_a_distinct_tone() {
        assert_eq!(
            probe_badge(&probe(OpenF1TokenProbeResult::Ok)).tone,
            BadgeTone::Ready
        );
        assert_eq!(
            probe_badge(&probe(OpenF1TokenProbeResult::Unauthorized)).tone,
            BadgeTone::Missing
        );
        assert_eq!(
            probe_badge(&probe(OpenF1TokenProbeResult::Unreachable)).tone,
            BadgeTone::Missing
        );
        assert_eq!(
            probe_badge(&probe(OpenF1TokenProbeResult::Invalid)).tone,
            BadgeTone::Degraded
        );
    }

    #[test]
    fn distinguishes_a_rejected_token_from_an_unreachable_api() {
        assert_eq!(
            probe_badge(&probe(OpenF1TokenProbeResult::Unauthorized)).label,
            "REJECTED"
        );
        assert_eq!(
            probe_badge(&probe(OpenF1TokenProbeResult::Unreachable)).label,
            "UNREACHABLE"
        );
    }

    #[test]
    fn prefers_the_servers_message() {
        assert_eq!(
            save_error_message(Some("token must not be blank")),
            "token must not be blank"
        );
        assert_eq!(save_error_message(Some("plain failure")), "plain failure");
    }

    #[test]
    fn falls_back_when_there_is_nothing_useful_to_show() {
        assert_eq!(save_error_message(Some("   ")), "Could not save the token.");
        assert_eq!(save_error_message(None), "Could not save the token.");
    }
}
