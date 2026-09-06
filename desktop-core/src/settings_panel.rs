//! Port of `frontend/src/lib/settingsPanel.ts`, reshaped for the OpenF1 login.

use crate::replay_quality::{BadgeTone, ChannelBadge};
use serde::{Deserialize, Serialize};

// The backend's `api::settings::OpenF1LoginSettings`/`OpenF1LoginProbe` are
// response-only types with private fields, so the client-side wire shape is
// mirrored here (matching `shared/types/api.ts`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenF1AuthSource {
    Settings,
    Env,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenF1LoginSettings {
    pub configured: bool,
    /// The saved account's username. Never accompanied by the password.
    pub username: Option<String>,
    pub source: OpenF1AuthSource,
    /// True when INTERVAL_OPENF1_LIVE_TOKEN is also set.
    pub env_token_present: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenF1LoginProbeResult {
    Ok,
    Unauthorized,
    Unreachable,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenF1LoginProbe {
    pub result: OpenF1LoginProbeResult,
    pub message: String,
}

/// Both halves are needed; OpenF1 generates the password, so it is not trimmed and
/// only an all-blank one counts as missing.
pub fn is_submittable_login(username: &str, password: &str) -> bool {
    !username.trim().is_empty() && !password.trim().is_empty()
}

/// One line describing what the backend is currently using, and where it came from.
pub fn login_source_line(settings: &OpenF1LoginSettings) -> String {
    match settings.source {
        OpenF1AuthSource::Settings => match settings.username.as_deref() {
            Some(username) if !username.trim().is_empty() => {
                format!("Signed in as {username}")
            }
            _ => "Signed in".to_string(),
        },
        OpenF1AuthSource::Env => {
            "Using INTERVAL_OPENF1_LIVE_TOKEN from the environment".to_string()
        }
        OpenF1AuthSource::None => "Not signed in".to_string(),
    }
}

/// Shown only when a saved login is shadowing an environment token. Without this the
/// user edits `.env`, sees nothing change, and has no way to find out why.
pub fn env_override_notice(settings: &OpenF1LoginSettings) -> Option<&'static str> {
    if settings.source != OpenF1AuthSource::Settings || !settings.env_token_present {
        return None;
    }
    Some(
        "INTERVAL_OPENF1_LIVE_TOKEN is also set. The login saved here takes precedence; sign out to use the environment token.",
    )
}

pub fn probe_badge(probe: &OpenF1LoginProbe) -> ChannelBadge {
    let (label, ready, tone) = match probe.result {
        OpenF1LoginProbeResult::Ok => ("LOGIN OK", true, BadgeTone::Ready),
        OpenF1LoginProbeResult::Unauthorized => ("REJECTED", false, BadgeTone::Missing),
        OpenF1LoginProbeResult::Unreachable => ("UNREACHABLE", false, BadgeTone::Missing),
        OpenF1LoginProbeResult::Invalid => ("INVALID", false, BadgeTone::Degraded),
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
        _ => "Could not save the login.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SettingsOverrides {
        username: Option<String>,
        source: OpenF1AuthSource,
        env_token_present: bool,
        configured: bool,
    }

    impl Default for SettingsOverrides {
        fn default() -> Self {
            Self {
                username: Some("me@example.com".to_string()),
                source: OpenF1AuthSource::Settings,
                env_token_present: false,
                configured: true,
            }
        }
    }

    fn settings(overrides: SettingsOverrides) -> OpenF1LoginSettings {
        OpenF1LoginSettings {
            configured: overrides.configured,
            username: overrides.username,
            source: overrides.source,
            env_token_present: overrides.env_token_present,
            path: Some("/config/interval/settings.json".to_string()),
        }
    }

    fn probe(result: OpenF1LoginProbeResult) -> OpenF1LoginProbe {
        OpenF1LoginProbe {
            result,
            message: "message".to_string(),
        }
    }

    #[test]
    fn accepts_a_complete_login() {
        assert!(is_submittable_login("me@example.com", "pw"));
        // A generated password may legitimately carry surrounding whitespace.
        assert!(is_submittable_login("me@example.com", " pw "));
    }

    #[test]
    fn rejects_a_login_with_either_half_blank() {
        assert!(!is_submittable_login("", "pw"));
        assert!(!is_submittable_login("   ", "pw"));
        assert!(!is_submittable_login("me@example.com", ""));
        assert!(!is_submittable_login("me@example.com", "   "));
    }

    #[test]
    fn names_the_signed_in_account() {
        assert_eq!(
            login_source_line(&settings(SettingsOverrides::default())),
            "Signed in as me@example.com"
        );
    }

    #[test]
    fn names_the_environment_variable_when_that_is_what_is_in_use() {
        assert_eq!(
            login_source_line(&settings(SettingsOverrides {
                source: OpenF1AuthSource::Env,
                username: None,
                ..Default::default()
            })),
            "Using INTERVAL_OPENF1_LIVE_TOKEN from the environment"
        );
    }

    #[test]
    fn reports_when_nothing_is_configured() {
        assert_eq!(
            login_source_line(&settings(SettingsOverrides {
                source: OpenF1AuthSource::None,
                username: None,
                configured: false,
                ..Default::default()
            })),
            "Not signed in"
        );
    }

    #[test]
    fn warns_only_when_a_saved_login_is_shadowing_an_environment_token() {
        assert!(env_override_notice(&settings(SettingsOverrides {
            source: OpenF1AuthSource::Settings,
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
                source: OpenF1AuthSource::Settings,
                env_token_present: false,
                ..Default::default()
            })),
            None
        );
        assert_eq!(
            env_override_notice(&settings(SettingsOverrides {
                source: OpenF1AuthSource::Env,
                env_token_present: true,
                ..Default::default()
            })),
            None
        );
        assert_eq!(
            env_override_notice(&settings(SettingsOverrides {
                source: OpenF1AuthSource::None,
                env_token_present: false,
                ..Default::default()
            })),
            None
        );
    }

    #[test]
    fn maps_each_probe_result_to_a_distinct_tone() {
        assert_eq!(
            probe_badge(&probe(OpenF1LoginProbeResult::Ok)).tone,
            BadgeTone::Ready
        );
        assert_eq!(
            probe_badge(&probe(OpenF1LoginProbeResult::Unauthorized)).tone,
            BadgeTone::Missing
        );
        assert_eq!(
            probe_badge(&probe(OpenF1LoginProbeResult::Unreachable)).tone,
            BadgeTone::Missing
        );
        assert_eq!(
            probe_badge(&probe(OpenF1LoginProbeResult::Invalid)).tone,
            BadgeTone::Degraded
        );
    }

    #[test]
    fn distinguishes_a_rejected_login_from_an_unreachable_api() {
        assert_eq!(
            probe_badge(&probe(OpenF1LoginProbeResult::Unauthorized)).label,
            "REJECTED"
        );
        assert_eq!(
            probe_badge(&probe(OpenF1LoginProbeResult::Unreachable)).label,
            "UNREACHABLE"
        );
    }

    #[test]
    fn prefers_the_servers_message() {
        assert_eq!(
            save_error_message(Some("password must not be blank")),
            "password must not be blank"
        );
        assert_eq!(save_error_message(Some("plain failure")), "plain failure");
    }

    #[test]
    fn falls_back_when_there_is_nothing_useful_to_show() {
        assert_eq!(save_error_message(Some("   ")), "Could not save the login.");
        assert_eq!(save_error_message(None), "Could not save the login.");
    }
}
