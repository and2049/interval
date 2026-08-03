use crate::{
    connectors::openf1_historical::RawEndpoint,
    domain::{Meeting, Session},
    normalization,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION},
    Url,
};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::Mutex;

const DEFAULT_BASE_URL: &str = "https://api.openf1.org/v1/";
const OPENF1_LIVE_REQUEST_TIMEOUT_SECONDS: u64 = 10;
const OPENF1_LIVE_REQUEST_INTERVAL_MS: u64 = 100;
const LIVE_WINDOW_PADDING_MINUTES: i64 = 30;
const LIVE_ENDPOINTS: &[LiveEndpointSpec] = &[
    LiveEndpointSpec {
        name: "drivers",
        cadence_ms: 30_000,
        incremental_field: None,
    },
    LiveEndpointSpec {
        name: "laps",
        cadence_ms: 2_000,
        incremental_field: None,
    },
    LiveEndpointSpec {
        name: "intervals",
        cadence_ms: 1_000,
        incremental_field: Some("date"),
    },
    LiveEndpointSpec {
        name: "position",
        cadence_ms: 500,
        incremental_field: Some("date"),
    },
    LiveEndpointSpec {
        name: "location",
        cadence_ms: 500,
        incremental_field: Some("date"),
    },
    LiveEndpointSpec {
        name: "pit",
        cadence_ms: 2_000,
        incremental_field: Some("date"),
    },
    LiveEndpointSpec {
        name: "race_control",
        cadence_ms: 2_000,
        incremental_field: Some("date"),
    },
    LiveEndpointSpec {
        name: "stints",
        cadence_ms: 5_000,
        incremental_field: None,
    },
    LiveEndpointSpec {
        name: "weather",
        cadence_ms: 10_000,
        incremental_field: Some("date"),
    },
    LiveEndpointSpec {
        name: "session_result",
        cadence_ms: 10_000,
        incremental_field: None,
    },
];

#[derive(Clone)]
pub struct OpenF1LiveClient {
    http: reqwest::Client,
    config: OpenF1LiveConfig,
    config_error: Option<String>,
    request_limiter: Arc<Mutex<LiveRequestLimiter>>,
}

#[derive(Debug, Default)]
struct LiveRequestLimiter {
    next_request: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct OpenF1LiveConfig {
    pub enabled: bool,
    pub base_url: Url,
    pub token: Option<String>,
    pub auth_header: String,
}

#[derive(Debug, Clone)]
pub struct ActiveLiveSession {
    pub session: Session,
    pub meeting: Option<Meeting>,
}

#[derive(Debug, Clone)]
pub struct LiveSessionDiscovery {
    pub current: Option<ActiveLiveSession>,
    pub next: Option<ActiveLiveSession>,
}

#[derive(Debug, Clone)]
pub struct LiveEndpointSpec {
    pub name: &'static str,
    pub cadence_ms: i64,
    pub incremental_field: Option<&'static str>,
}

pub fn live_endpoint_cadence_seconds(endpoint: &str) -> f64 {
    LIVE_ENDPOINTS
        .iter()
        .find(|spec| spec.name == endpoint)
        .map(|spec| spec.cadence_ms as f64 / 1_000.0)
        .unwrap_or(1.0)
}

impl Default for OpenF1LiveClient {
    fn default() -> Self {
        Self::from_env()
    }
}

impl OpenF1LiveClient {
    pub fn from_env() -> Self {
        let enabled =
            live_enabled_from_env_value(std::env::var("INTERVAL_OPENF1_LIVE_ENABLED").ok());
        let configured_base_url = std::env::var("INTERVAL_OPENF1_LIVE_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let (base_url, config_error) = live_base_url_from_env_value(configured_base_url);
        let token = std::env::var("INTERVAL_OPENF1_LIVE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let auth_header = std::env::var("INTERVAL_OPENF1_LIVE_AUTH_HEADER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| AUTHORIZATION.as_str().to_string());

        Self {
            http: live_http_client(),
            config: OpenF1LiveConfig {
                enabled,
                base_url,
                token,
                auth_header,
            },
            config_error,
            request_limiter: Arc::new(Mutex::new(LiveRequestLimiter::default())),
        }
    }

    pub fn with_config(config: OpenF1LiveConfig) -> Self {
        let mut config = config;
        config.base_url = with_trailing_slash(config.base_url);
        Self {
            http: live_http_client(),
            config,
            config_error: None,
            request_limiter: Arc::new(Mutex::new(LiveRequestLimiter::default())),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_config_error(config: OpenF1LiveConfig, error: impl Into<String>) -> Self {
        Self {
            http: live_http_client(),
            config,
            config_error: Some(error.into()),
            request_limiter: Arc::new(Mutex::new(LiveRequestLimiter::default())),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn endpoint_specs(&self) -> &'static [LiveEndpointSpec] {
        LIVE_ENDPOINTS
    }

    pub async fn current_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ActiveLiveSession>, OpenF1LiveError> {
        Ok(self.current_and_next_session(now).await?.current)
    }

    pub async fn next_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ActiveLiveSession>, OpenF1LiveError> {
        Ok(self.current_and_next_session(now).await?.next)
    }

    pub async fn current_and_next_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<LiveSessionDiscovery, OpenF1LiveError> {
        if !self.enabled() {
            return Ok(LiveSessionDiscovery {
                current: None,
                next: None,
            });
        }
        self.ensure_valid_config()?;

        let year = now.year();
        let meetings_payload = self
            .fetch_endpoint("meetings", &[("year".to_string(), year.to_string())])
            .await?;
        let mut meetings = normalization::meetings_from_openf1(meetings_payload)?;
        let sessions_payload = self
            .fetch_endpoint("sessions", &[("year".to_string(), year.to_string())])
            .await?;
        let mut sessions = normalization::race_sessions_from_openf1(sessions_payload)?;

        if next_live_session_from_schedule(&sessions, &meetings, now).is_none()
            && chrono::Datelike::month(&now) >= 11
        {
            let next_year = year + 1;
            let next_meetings = self
                .fetch_endpoint("meetings", &[("year".to_string(), next_year.to_string())])
                .await;
            let next_sessions = self
                .fetch_endpoint("sessions", &[("year".to_string(), next_year.to_string())])
                .await;
            if let (Ok(meetings_payload), Ok(sessions_payload)) = (next_meetings, next_sessions) {
                meetings.extend(normalization::meetings_from_openf1(meetings_payload)?);
                sessions.extend(normalization::race_sessions_from_openf1(sessions_payload)?);
            }
        }

        Ok(LiveSessionDiscovery {
            current: current_live_session_from_schedule(&sessions, &meetings, now),
            next: next_live_session_from_schedule(&sessions, &meetings, now),
        })
    }

    pub async fn fetch_live_bundle(
        &self,
        session_key: i64,
    ) -> Result<Vec<RawEndpoint>, OpenF1LiveError> {
        if !self.enabled() {
            return Err(OpenF1LiveError::Disabled);
        }
        self.ensure_valid_config()?;

        let mut out = Vec::with_capacity(LIVE_ENDPOINTS.len());
        for spec in LIVE_ENDPOINTS {
            let payload = self
                .fetch_endpoint(
                    spec.name,
                    &[("session_key".to_string(), session_key.to_string())],
                )
                .await
                .unwrap_or_else(|_| Value::Array(vec![]));
            out.push(RawEndpoint {
                endpoint: spec.name.to_string(),
                session_key,
                payload,
            });
        }
        Ok(out)
    }

    pub async fn fetch_live_bundle_endpoint(
        &self,
        session_key: i64,
        endpoint: &str,
    ) -> Result<RawEndpoint, OpenF1LiveError> {
        if !self.enabled() {
            return Err(OpenF1LiveError::Disabled);
        }
        self.ensure_valid_config()?;
        let payload = self
            .fetch_endpoint(
                endpoint,
                &[("session_key".to_string(), session_key.to_string())],
            )
            .await?;
        Ok(RawEndpoint {
            endpoint: endpoint.to_string(),
            session_key,
            payload,
        })
    }

    pub async fn fetch_live_bundle_endpoint_since(
        &self,
        session_key: i64,
        endpoint: &str,
        since: DateTime<Utc>,
    ) -> Result<RawEndpoint, OpenF1LiveError> {
        if !self.enabled() {
            return Err(OpenF1LiveError::Disabled);
        }
        self.ensure_valid_config()?;
        let params = live_endpoint_params(session_key, endpoint, Some(since));
        let payload = self.fetch_endpoint(endpoint, &params).await?;
        Ok(RawEndpoint {
            endpoint: endpoint.to_string(),
            session_key,
            payload,
        })
    }

    async fn fetch_endpoint(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<Value, OpenF1LiveError> {
        let mut url = self.config.base_url.join(endpoint)?;
        url.query_pairs_mut().extend_pairs(
            params
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );

        self.request_limiter.lock().await.wait_turn().await;
        let response = self
            .http
            .get(url)
            .headers(self.auth_headers()?)
            .send()
            .await?
            .error_for_status()?;
        let payload = response.json::<Value>().await?;
        if !payload.is_array() {
            return Err(OpenF1LiveError::Normalize(anyhow::anyhow!(
                "OpenF1 {endpoint} returned a non-array payload"
            )));
        }
        Ok(payload)
    }

    fn auth_headers(&self) -> Result<HeaderMap, OpenF1LiveError> {
        let mut headers = HeaderMap::new();
        let Some(token) = &self.config.token else {
            return Ok(headers);
        };
        let name = HeaderName::from_bytes(self.config.auth_header.as_bytes())
            .map_err(|_| OpenF1LiveError::InvalidAuthHeader(self.config.auth_header.clone()))?;
        let value = if name == AUTHORIZATION {
            authorization_header_value(token)
        } else {
            token.clone()
        };
        headers.insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| OpenF1LiveError::InvalidAuthHeaderValue)?,
        );
        Ok(headers)
    }

    fn ensure_valid_config(&self) -> Result<(), OpenF1LiveError> {
        if let Some(error) = &self.config_error {
            return Err(OpenF1LiveError::Config(error.clone()));
        }
        Ok(())
    }
}

impl LiveRequestLimiter {
    async fn wait_turn(&mut self) {
        let interval = if cfg!(test) {
            Duration::ZERO
        } else {
            Duration::from_millis(OPENF1_LIVE_REQUEST_INTERVAL_MS)
        };
        let now = Instant::now();
        if let Some(next) = self.next_request.filter(|next| *next > now) {
            tokio::time::sleep(next.duration_since(now)).await;
        }
        self.next_request = Some(Instant::now() + interval);
    }
}

fn live_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(OPENF1_LIVE_REQUEST_TIMEOUT_SECONDS))
        .build()
        .expect("valid OpenF1 live HTTP client")
}

fn authorization_header_value(token: &str) -> String {
    if token
        .trim_start()
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        token.trim_start().to_string()
    } else {
        format!("Bearer {token}")
    }
}

fn live_window_rank(session: &Session, now: DateTime<Utc>) -> Option<(u8, i64)> {
    let Ok(start) = DateTime::parse_from_rfc3339(&session.start_time) else {
        return None;
    };
    let start = start.with_timezone(&Utc);
    let end = live_session_end_or_default(start, &session.end_time);
    let close = end + ChronoDuration::minutes(LIVE_WINDOW_PADDING_MINUTES);
    if now < start || now > close {
        return None;
    }
    if now >= start && now <= end {
        return Some((0, now.signed_duration_since(start).num_milliseconds().abs()));
    }
    Some((1, now.signed_duration_since(end).num_milliseconds()))
}

fn live_session_end_or_default(start: DateTime<Utc>, end_time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(end_time)
        .map(|value| value.with_timezone(&Utc))
        .ok()
        .filter(|end| *end > start)
        .unwrap_or_else(|| start + ChronoDuration::hours(3))
}

fn live_base_url_from_env_value(value: Option<String>) -> (Url, Option<String>) {
    match value {
        Some(value) => match Url::parse(&value) {
            Ok(url) => (with_trailing_slash(url), None),
            Err(error) => (
                Url::parse(DEFAULT_BASE_URL).expect("valid OpenF1 live base URL"),
                Some(format!("INTERVAL_OPENF1_LIVE_BASE_URL is invalid: {error}")),
            ),
        },
        None => (
            Url::parse(DEFAULT_BASE_URL).expect("valid OpenF1 live base URL"),
            None,
        ),
    }
}

fn live_enabled_from_env_value(value: Option<String>) -> bool {
    !matches!(
        value
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("0" | "false" | "no" | "off")
    )
}

fn with_trailing_slash(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}

fn live_endpoint_params(
    session_key: i64,
    endpoint: &str,
    since: Option<DateTime<Utc>>,
) -> Vec<(String, String)> {
    let mut params = vec![("session_key".to_string(), session_key.to_string())];
    if let Some((field, since)) = since.and_then(|since| {
        LIVE_ENDPOINTS
            .iter()
            .find(|spec| spec.name == endpoint)
            .and_then(|spec| spec.incremental_field)
            .map(|field| (field, since))
    }) {
        params.push((format!("{field}>="), since.to_rfc3339()));
    }
    params
}

fn current_live_session_from_schedule(
    sessions: &[Session],
    meetings: &[Meeting],
    now: DateTime<Utc>,
) -> Option<ActiveLiveSession> {
    let mut sessions = sessions.to_vec();
    sessions.sort_by_key(|session| live_window_rank(session, now));
    sessions.into_iter().find_map(|session| {
        live_window_rank(&session, now)?;
        Some(active_live_session(session, meetings))
    })
}

fn next_live_session_from_schedule(
    sessions: &[Session],
    meetings: &[Meeting],
    now: DateTime<Utc>,
) -> Option<ActiveLiveSession> {
    let mut sessions = sessions.to_vec();
    sessions.sort_by_key(|session| session.start_time.clone());
    sessions.into_iter().find_map(|session| {
        let start = DateTime::parse_from_rfc3339(&session.start_time)
            .ok()?
            .with_timezone(&Utc);
        if start <= now {
            return None;
        }
        Some(active_live_session(session, meetings))
    })
}

fn active_live_session(session: Session, meetings: &[Meeting]) -> ActiveLiveSession {
    let meeting = meetings
        .iter()
        .find(|meeting| meeting.meeting_key == session.meeting_key)
        .cloned();
    ActiveLiveSession { session, meeting }
}

#[derive(Debug, Error)]
pub enum OpenF1LiveError {
    #[error("OpenF1 live mode is disabled")]
    Disabled,
    #[error("OpenF1 live configuration error: {0}")]
    Config(String),
    #[error("invalid OpenF1 live url: {0}")]
    Url(#[from] url::ParseError),
    #[error("OpenF1 live request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("OpenF1 live normalization failed: {0}")]
    Normalize(#[from] anyhow::Error),
    #[error("invalid OpenF1 live auth header: {0}")]
    InvalidAuthHeader(String),
    #[error("invalid OpenF1 live auth header value")]
    InvalidAuthHeaderValue,
}

trait DateYear {
    fn year(&self) -> i32;
}

impl DateYear for DateTime<Utc> {
    fn year(&self) -> i32 {
        chrono::Datelike::year(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionType;

    #[test]
    fn live_window_does_not_mark_pre_session_padding_active() {
        let session = Session {
            session_key: 1,
            meeting_key: 2,
            year: 2026,
            name: "Race".to_string(),
            session_type: SessionType::Race,
            start_time: "2026-06-28T13:00:00Z".to_string(),
            end_time: "2026-06-28T15:00:00Z".to_string(),
            total_laps: 0,
        };

        let now = DateTime::parse_from_rfc3339("2026-06-28T12:45:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert!(live_window_rank(&session, now).is_none());
    }

    #[test]
    fn next_live_session_from_schedule_includes_pre_session_candidates() {
        let sprint = session(
            1,
            "Sprint",
            SessionType::Sprint,
            "2026-06-28T12:00:00Z",
            "2026-06-28T13:00:00Z",
        );
        let race = session(
            2,
            "Race",
            SessionType::Race,
            "2026-06-28T13:20:00Z",
            "2026-06-28T15:20:00Z",
        );
        let now = DateTime::parse_from_rfc3339("2026-06-28T13:05:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let next = next_live_session_from_schedule(&[sprint, race], &[], now).unwrap();

        assert_eq!(next.session.session_key, 2);
    }

    #[test]
    fn live_window_rank_prefers_in_progress_session() {
        let pre_race = session(
            2,
            "Race",
            SessionType::Race,
            "2026-06-28T13:20:00Z",
            "2026-06-28T15:20:00Z",
        );
        let live_sprint = session(
            1,
            "Sprint",
            SessionType::Sprint,
            "2026-06-28T12:00:00Z",
            "2026-06-28T13:10:00Z",
        );
        let now = DateTime::parse_from_rfc3339("2026-06-28T13:05:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert!(live_window_rank(&live_sprint, now).is_some());
        assert!(live_window_rank(&pre_race, now).is_none());
    }

    #[test]
    fn live_window_rank_defaults_when_end_time_is_before_start() {
        let race = session(
            1,
            "Race",
            SessionType::Race,
            "2026-06-28T13:00:00Z",
            "2026-06-28T12:00:00Z",
        );
        let now = DateTime::parse_from_rfc3339("2026-06-28T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert!(live_window_rank(&race, now).is_some());
    }

    #[test]
    fn next_live_session_from_schedule_picks_earliest_future_session() {
        let now = DateTime::parse_from_rfc3339("2026-06-28T13:05:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let sessions = vec![
            session(
                3,
                "Race",
                SessionType::Race,
                "2026-06-29T13:00:00Z",
                "2026-06-29T15:00:00Z",
            ),
            session(
                1,
                "Sprint",
                SessionType::Sprint,
                "2026-06-28T12:00:00Z",
                "2026-06-28T13:00:00Z",
            ),
            session(
                2,
                "Race",
                SessionType::Race,
                "2026-06-28T14:00:00Z",
                "2026-06-28T16:00:00Z",
            ),
        ];

        let next = next_live_session_from_schedule(&sessions, &[], now).unwrap();

        assert_eq!(next.session.session_key, 2);
    }

    #[tokio::test]
    async fn invalid_live_configuration_is_reported_before_requests() {
        let client = OpenF1LiveClient::with_config_error(
            OpenF1LiveConfig {
                enabled: true,
                base_url: Url::parse(DEFAULT_BASE_URL).unwrap(),
                token: None,
                auth_header: AUTHORIZATION.as_str().to_string(),
            },
            "INTERVAL_OPENF1_LIVE_BASE_URL is invalid",
        );

        let error = client.current_session(Utc::now()).await.unwrap_err();

        assert!(error
            .to_string()
            .contains("OpenF1 live configuration error"));
    }

    #[test]
    fn invalid_env_base_url_records_configuration_error() {
        let (base_url, error) = live_base_url_from_env_value(Some("not a valid url".to_string()));

        assert_eq!(base_url.as_str(), DEFAULT_BASE_URL);
        assert!(error
            .as_deref()
            .is_some_and(|message| message.contains("INTERVAL_OPENF1_LIVE_BASE_URL")));
    }

    #[test]
    fn live_enabled_defaults_to_on_unless_explicitly_disabled() {
        assert!(live_enabled_from_env_value(None));
        assert!(live_enabled_from_env_value(Some("true".to_string())));
        assert!(live_enabled_from_env_value(Some("".to_string())));
        assert!(!live_enabled_from_env_value(Some("false".to_string())));
        assert!(!live_enabled_from_env_value(Some("0".to_string())));
        assert!(!live_enabled_from_env_value(Some("off".to_string())));
    }

    #[test]
    fn configured_base_url_keeps_version_path_when_trailing_slash_is_missing() {
        let (base_url, error) =
            live_base_url_from_env_value(Some("https://example.test/v1".to_string()));

        assert_eq!(
            base_url.join("sessions").unwrap().as_str(),
            "https://example.test/v1/sessions"
        );
        assert!(error.is_none());
    }

    #[test]
    fn explicit_config_keeps_version_path_when_trailing_slash_is_missing() {
        let client = OpenF1LiveClient::with_config(OpenF1LiveConfig {
            enabled: true,
            base_url: Url::parse("https://example.test/v1").unwrap(),
            token: None,
            auth_header: AUTHORIZATION.as_str().to_string(),
        });

        assert_eq!(
            client.config.base_url.join("sessions").unwrap().as_str(),
            "https://example.test/v1/sessions"
        );
    }

    #[test]
    fn live_endpoint_params_add_incremental_date_filter_only_for_timestamped_endpoints() {
        let since = DateTime::parse_from_rfc3339("2026-06-28T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(
            live_endpoint_params(42, "location", Some(since)),
            vec![
                ("session_key".to_string(), "42".to_string()),
                (
                    "date>=".to_string(),
                    "2026-06-28T13:00:00+00:00".to_string()
                )
            ]
        );
        assert_eq!(
            live_endpoint_params(42, "drivers", Some(since)),
            vec![("session_key".to_string(), "42".to_string())]
        );
    }

    #[test]
    fn live_endpoint_cadence_seconds_uses_endpoint_specs() {
        assert_eq!(live_endpoint_cadence_seconds("location"), 0.5);
        assert_eq!(live_endpoint_cadence_seconds("weather"), 10.0);
        assert_eq!(live_endpoint_cadence_seconds("unknown"), 1.0);
    }

    #[test]
    fn authorization_header_value_accepts_raw_or_prefixed_tokens() {
        assert_eq!(authorization_header_value("abc123"), "Bearer abc123");
        assert_eq!(authorization_header_value("Bearer abc123"), "Bearer abc123");
        assert_eq!(authorization_header_value("bearer abc123"), "bearer abc123");
        assert_eq!(
            authorization_header_value("  Bearer abc123"),
            "Bearer abc123"
        );
    }

    fn session(
        session_key: i64,
        name: &str,
        session_type: SessionType,
        start_time: &str,
        end_time: &str,
    ) -> Session {
        Session {
            session_key,
            meeting_key: 2,
            year: 2026,
            name: name.to_string(),
            session_type,
            start_time: start_time.to_string(),
            end_time: end_time.to_string(),
            total_laps: 0,
        }
    }
}
