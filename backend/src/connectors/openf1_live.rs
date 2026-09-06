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
use serde::Deserialize;
use serde_json::Value;
use std::{
    fmt,
    sync::{Arc, PoisonError, RwLock},
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::Mutex;

const DEFAULT_BASE_URL: &str = "https://api.openf1.org/v1/";
const OPENF1_LIVE_REQUEST_TIMEOUT_SECONDS: u64 = 10;
/// OpenF1's sponsor tier allows 6 requests per second *and* 60 per minute
/// (<https://openf1.org/>, "Pricing"). Both are enforced by `LiveRequestLimiter`: the
/// interval spaces bursts, the per-minute budget is the one that actually binds. It is
/// kept a little under 60 so the schedule fetch and token exchange, which share the
/// limiter, do not tip a full minute into 429s.
const OPENF1_LIVE_REQUEST_INTERVAL_MS: u64 = 175;
const OPENF1_LIVE_REQUESTS_PER_MINUTE: usize = 56;
const OPENF1_LIVE_RATE_WINDOW: Duration = Duration::from_secs(60);
/// How long every poller stands down after a 429 with no `Retry-After` header.
const OPENF1_LIVE_DEFAULT_RATE_LIMIT_PAUSE: Duration = Duration::from_secs(10);
/// Test builds run the live cadences this much faster so the API tests, which time
/// their assertions against them, finish in seconds rather than minutes.
const TEST_CADENCE_SCALE: f64 = 0.1;
/// Path of OpenF1's token endpoint, relative to the API host (not the `/v1/` root).
const TOKEN_PATH: &str = "/token";
/// OpenF1 tokens last an hour. Re-exchange this long before the stated expiry so a
/// poll never goes out with a token that dies in flight.
const TOKEN_REFRESH_MARGIN_SECONDS: u64 = 120;
const DEFAULT_TOKEN_LIFETIME_SECONDS: u64 = 3_600;
const SCHEDULE_CACHE_SECONDS: u64 = 60;
const LIVE_WINDOW_PADDING_MINUTES: i64 = 30;
// The cadences below add up to ~54 requests per minute for one live session, under the
// 60/min budget; `live_endpoint_cadences_stay_under_the_openf1_rate_limit` guards the
// sum and the request limiter is the hard cap if they ever drift over. The fast feeds
// (intervals, position, location) get the largest shares; race control comes next so
// flags and safety cars are not the slowest thing on screen; the rest change slowly.
const LIVE_ENDPOINTS: &[LiveEndpointSpec] = &[
    LiveEndpointSpec {
        name: "drivers",
        cadence_ms: 60_000,
        incremental_field: None,
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "laps",
        cadence_ms: 12_000,
        incremental_field: None,
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "intervals",
        cadence_ms: 5_000,
        incremental_field: Some("date"),
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "position",
        cadence_ms: 6_000,
        incremental_field: Some("date"),
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "location",
        cadence_ms: 5_000,
        incremental_field: Some("date"),
        // ~80 rows/s across the field: a whole session is hundreds of thousands of
        // rows and OpenF1 refuses it (422 "asking for too much data at once"). Four
        // minutes is a couple of laps, enough for map geometry and retirement
        // inference, and sits just under the 20k rows the registry keeps anyway.
        initial_window_seconds: Some(240),
    },
    LiveEndpointSpec {
        name: "pit",
        cadence_ms: 30_000,
        incremental_field: Some("date"),
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "race_control",
        cadence_ms: 8_000,
        incremental_field: Some("date"),
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "stints",
        cadence_ms: 30_000,
        incremental_field: None,
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "weather",
        cadence_ms: 60_000,
        incremental_field: Some("date"),
        initial_window_seconds: None,
    },
    LiveEndpointSpec {
        name: "session_result",
        cadence_ms: 60_000,
        incremental_field: None,
        initial_window_seconds: None,
    },
];

#[derive(Clone)]
pub struct OpenF1LiveClient {
    http: reqwest::Client,
    // Shared so a token saved from the settings UI reaches clones that already exist:
    // the registry clones this client per endpoint fetch and axum clones AppState per
    // request, but every clone descends from one instance, so they share this Arc and a
    // live poll already in flight picks up the new token on its next request.
    // std::sync::RwLock deliberately, not tokio's: its guard is !Send, so holding one
    // across an await is a compile error rather than a silent bug.
    config: Arc<RwLock<OpenF1LiveConfig>>,
    config_error: Option<String>,
    request_limiter: Arc<Mutex<LiveRequestLimiter>>,
    schedule_cache: Arc<Mutex<Option<ScheduleCacheEntry>>>,
    // The bearer token issued for the configured login. Shared for the same reason as
    // `config`: one exchange per hour for the whole process, not one per clone. A tokio
    // mutex because it is held across the exchange request, which also serialises the
    // ten endpoint pollers that would otherwise all notice expiry at the same moment.
    issued_token: Arc<Mutex<Option<IssuedToken>>>,
}

#[derive(Debug, Clone)]
struct IssuedToken {
    access_token: String,
    expires_at: Instant,
}

/// A bearer value ready to send, plus whether it came out of the cache. Only a cached
/// token is worth re-exchanging after a 401: a token OpenF1 just issued, or a static
/// one from the environment, will not get better by asking again.
struct BearerToken {
    value: String,
    reused: bool,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    /// OpenF1 documents this as the string `"3600"`, while the OAuth norm is a number.
    /// Accept either; anything unparseable falls back to the default lifetime.
    #[serde(default, deserialize_with = "deserialize_lenient_seconds")]
    expires_in: Option<u64>,
}

fn deserialize_lenient_seconds<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().map(|seconds| seconds.max(0.0) as u64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }))
}

/// Paces every request to OpenF1 from this process: a minimum gap between requests, a
/// sliding one-minute budget, and a global stand-down after a 429.
#[derive(Debug)]
struct LiveRequestLimiter {
    interval: Duration,
    per_window: usize,
    window: Duration,
    next_request: Option<Instant>,
    paused_until: Option<Instant>,
    sent: std::collections::VecDeque<Instant>,
}

impl Default for LiveRequestLimiter {
    fn default() -> Self {
        if cfg!(test) {
            // Tests hit local mocks and must not be paced.
            Self::new(Duration::ZERO, usize::MAX, OPENF1_LIVE_RATE_WINDOW)
        } else {
            Self::new(
                Duration::from_millis(OPENF1_LIVE_REQUEST_INTERVAL_MS),
                OPENF1_LIVE_REQUESTS_PER_MINUTE,
                OPENF1_LIVE_RATE_WINDOW,
            )
        }
    }
}

#[derive(Debug, Clone)]
struct ScheduleCacheEntry {
    fetched_at: Instant,
    sessions: Vec<Session>,
    meetings: Vec<Meeting>,
}

#[derive(Debug, Clone)]
pub struct OpenF1LiveConfig {
    pub enabled: bool,
    pub base_url: Url,
    pub auth: OpenF1Auth,
    pub auth_header: String,
}

/// How requests to OpenF1 are authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenF1Auth {
    /// Unauthenticated. Only the free historical endpoints answer.
    None,
    /// A pre-issued bearer token, sent as-is. The environment escape hatch; OpenF1's own
    /// tokens expire after an hour, so this is for testing and compatible mirrors.
    Token(String),
    /// An OpenF1 account. Exchanged for a one-hour bearer token on first use and again
    /// whenever that token expires or is rejected.
    Login(OpenF1Login),
}

impl OpenF1Auth {
    pub fn is_configured(&self) -> bool {
        !matches!(self, OpenF1Auth::None)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OpenF1Login {
    pub username: String,
    pub password: String,
}

// Hand-written so a debug-logged config can never print the password.
impl fmt::Debug for OpenF1Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenF1Login")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
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

#[derive(Debug, Clone, Copy)]
pub struct LiveEndpointSpec {
    pub name: &'static str,
    pub cadence_ms: i64,
    pub incremental_field: Option<&'static str>,
    /// How far back the *first* fetch of a session reaches, for feeds too big to pull
    /// whole. Requires `incremental_field`. `None` fetches the entire session.
    pub initial_window_seconds: Option<i64>,
}

impl LiveEndpointSpec {
    /// The lower bound for the first fetch, if this feed is windowed.
    pub fn initial_since(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.initial_window_seconds
            .map(|seconds| now - chrono::Duration::seconds(seconds))
    }
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
        // A login saved through the settings UI takes precedence over the environment.
        let (auth, _source) = crate::settings::resolve_openf1_auth(
            &crate::settings::SettingsStore::default_location().load(),
            std::env::var("INTERVAL_OPENF1_LIVE_TOKEN").ok(),
        );
        let auth_header = std::env::var("INTERVAL_OPENF1_LIVE_AUTH_HEADER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| AUTHORIZATION.as_str().to_string());

        Self {
            http: live_http_client(),
            config: Arc::new(RwLock::new(OpenF1LiveConfig {
                enabled,
                base_url,
                auth,
                auth_header,
            })),
            config_error,
            request_limiter: Arc::new(Mutex::new(LiveRequestLimiter::default())),
            schedule_cache: Arc::new(Mutex::new(None)),
            issued_token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_config(config: OpenF1LiveConfig) -> Self {
        let mut config = config;
        config.base_url = with_trailing_slash(config.base_url);
        Self {
            http: live_http_client(),
            config: Arc::new(RwLock::new(config)),
            config_error: None,
            request_limiter: Arc::new(Mutex::new(LiveRequestLimiter::default())),
            schedule_cache: Arc::new(Mutex::new(None)),
            issued_token: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_config_error(config: OpenF1LiveConfig, error: impl Into<String>) -> Self {
        Self {
            http: live_http_client(),
            config: Arc::new(RwLock::new(config)),
            config_error: Some(error.into()),
            request_limiter: Arc::new(Mutex::new(LiveRequestLimiter::default())),
            schedule_cache: Arc::new(Mutex::new(None)),
            issued_token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config().enabled
    }

    /// Snapshot of the current config. Clones so no guard is ever held across an await.
    fn config(&self) -> OpenF1LiveConfig {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Applies new OpenF1 credentials to this client and every clone of it.
    ///
    /// Clearing the caches is not optional. The schedule is served for up to 60 seconds,
    /// so without this a user who fixes a bad login keeps seeing the same failure and
    /// concludes the fix did not work; and a token issued for the previous login must
    /// not outlive it.
    pub async fn set_auth(&self, auth: OpenF1Auth) {
        self.config
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .auth = auth;
        *self.issued_token.lock().await = None;
        *self.schedule_cache.lock().await = None;
    }

    /// One cheap authenticated request, for the settings panel's connection test.
    ///
    /// `session_key=latest` returns a single row, unlike the schedule fetch which pulls
    /// a whole year. With a login configured this also exercises the token exchange, so
    /// a wrong password surfaces here as `Unauthorized`. Deliberately ignores `enabled`:
    /// "does this login work" is a useful question even when live discovery is off.
    pub async fn probe(&self) -> Result<(), OpenF1LiveError> {
        self.ensure_valid_config()?;
        self.fetch_endpoint(
            "sessions",
            &[("session_key".to_string(), "latest".to_string())],
        )
        .await?;
        Ok(())
    }

    pub fn endpoint_specs(&self) -> Vec<LiveEndpointSpec> {
        LIVE_ENDPOINTS
            .iter()
            .map(|spec| {
                if cfg!(test) {
                    LiveEndpointSpec {
                        cadence_ms: (spec.cadence_ms as f64 * TEST_CADENCE_SCALE) as i64,
                        ..*spec
                    }
                } else {
                    *spec
                }
            })
            .collect()
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
        let (sessions, meetings) = self.cached_schedule(now).await?;

        Ok(LiveSessionDiscovery {
            current: current_live_session_from_schedule(&sessions, &meetings, now),
            next: next_live_session_from_schedule(&sessions, &meetings, now),
        })
    }

    /// Season schedules change rarely; caching them keeps `current()` polls
    /// from re-fetching the full year of meetings and sessions every time.
    async fn cached_schedule(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(Vec<Session>, Vec<Meeting>), OpenF1LiveError> {
        {
            let cache = self.schedule_cache.lock().await;
            if let Some(entry) = cache.as_ref() {
                if entry.fetched_at.elapsed() < Duration::from_secs(SCHEDULE_CACHE_SECONDS) {
                    return Ok((entry.sessions.clone(), entry.meetings.clone()));
                }
            }
        }

        let (sessions, meetings) = self.fetch_schedule(now).await?;
        *self.schedule_cache.lock().await = Some(ScheduleCacheEntry {
            fetched_at: Instant::now(),
            sessions: sessions.clone(),
            meetings: meetings.clone(),
        });
        Ok((sessions, meetings))
    }

    async fn fetch_schedule(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(Vec<Session>, Vec<Meeting>), OpenF1LiveError> {
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

        Ok((sessions, meetings))
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
        let now = Utc::now();
        for spec in LIVE_ENDPOINTS {
            let payload = self
                .fetch_endpoint(
                    spec.name,
                    &live_endpoint_params(session_key, spec.name, spec.initial_since(now)),
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

    /// The first fetch of a feed for a session: the whole session, or the trailing
    /// window the spec asks for.
    pub async fn fetch_initial_live_bundle_endpoint(
        &self,
        session_key: i64,
        spec: &LiveEndpointSpec,
    ) -> Result<RawEndpoint, OpenF1LiveError> {
        match spec.initial_since(Utc::now()) {
            Some(since) => {
                self.fetch_live_bundle_endpoint_since(session_key, spec.name, since)
                    .await
            }
            None => self.fetch_live_bundle_endpoint(session_key, spec.name).await,
        }
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
        let config = self.config();
        let url = live_endpoint_url(&config.base_url, endpoint, params)?;

        let mut retried = false;
        loop {
            let bearer = self.bearer_token(&config).await?;
            self.request_limiter.lock().await.wait_turn().await;
            let response = self
                .http
                .get(url.clone())
                .headers(auth_headers(&config, bearer.as_ref())?)
                .send()
                .await?;
            // Separate rejected credentials from an unreachable API. Without this both
            // collapse into a transport error and the UI cannot tell a bad login from
            // OpenF1 being down.
            let status = response.status();
            if is_auth_rejection(status) {
                // A cached token can be revoked, or expire early if OpenF1's clock and
                // ours disagree. Exchange the login once more and retry once; a second
                // rejection is a real one.
                if !retried && bearer.as_ref().is_some_and(|bearer| bearer.reused) {
                    *self.issued_token.lock().await = None;
                    retried = true;
                    continue;
                }
                return Err(OpenF1LiveError::Unauthorized(status.as_u16()));
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                // The minute's budget is spent. Stand every poller down rather than let
                // each of them discover the same thing and burn the next budget too.
                let pause = retry_after(response.headers())
                    .unwrap_or(OPENF1_LIVE_DEFAULT_RATE_LIMIT_PAUSE);
                self.request_limiter
                    .lock()
                    .await
                    .pause_until(Instant::now() + pause);
                return Err(OpenF1LiveError::RateLimited {
                    endpoint: endpoint.to_string(),
                    pause_seconds: pause.as_secs(),
                });
            }
            if status == reqwest::StatusCode::NOT_FOUND {
                // OpenF1 answers an empty result set with 404 rather than `[]`: a
                // `date>=` filter past the newest row, or `session_result` before the
                // session ends. Any other 404 is a wrong path and stays an error.
                let body = response.text().await.unwrap_or_default();
                if is_no_results_body(&body) {
                    return Ok(Value::Array(vec![]));
                }
                return Err(OpenF1LiveError::Status {
                    endpoint: endpoint.to_string(),
                    status: status.as_u16(),
                    body: body.chars().take(200).collect(),
                });
            }
            let response = response.error_for_status()?;
            let payload = response.json::<Value>().await?;
            if !payload.is_array() {
                return Err(OpenF1LiveError::Normalize(anyhow::anyhow!(
                    "OpenF1 {endpoint} returned a non-array payload"
                )));
            }
            return Ok(payload);
        }
    }

    /// The bearer value for the next request, exchanging the login for a fresh token
    /// when there is no usable one. Holds the token lock across the exchange so
    /// concurrent pollers wait for one exchange instead of each starting their own.
    async fn bearer_token(
        &self,
        config: &OpenF1LiveConfig,
    ) -> Result<Option<BearerToken>, OpenF1LiveError> {
        match &config.auth {
            OpenF1Auth::None => Ok(None),
            OpenF1Auth::Token(token) => Ok(Some(BearerToken {
                value: token.clone(),
                reused: false,
            })),
            OpenF1Auth::Login(login) => {
                let mut issued = self.issued_token.lock().await;
                if let Some(token) = issued
                    .as_ref()
                    .filter(|token| Instant::now() < token.expires_at)
                {
                    return Ok(Some(BearerToken {
                        value: token.access_token.clone(),
                        reused: true,
                    }));
                }
                let token = self.exchange_login(config, login).await?;
                let value = token.access_token.clone();
                *issued = Some(token);
                Ok(Some(BearerToken {
                    value,
                    reused: false,
                }))
            }
        }
    }

    /// `POST /token` with the login as a form body, per <https://openf1.org/auth.html>.
    async fn exchange_login(
        &self,
        config: &OpenF1LiveConfig,
        login: &OpenF1Login,
    ) -> Result<IssuedToken, OpenF1LiveError> {
        let url = token_url(&config.base_url)?;
        self.request_limiter.lock().await.wait_turn().await;
        let response = self
            .http
            .post(url)
            .form(&[
                ("username", login.username.as_str()),
                ("password", login.password.as_str()),
            ])
            .send()
            .await?;
        let status = response.status();
        // FastAPI-style servers answer a wrong password with 401 and a malformed login
        // with 400 or 422; every one of them means "this login does not work".
        if is_auth_rejection(status)
            || status == reqwest::StatusCode::BAD_REQUEST
            || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
        {
            return Err(OpenF1LiveError::Unauthorized(status.as_u16()));
        }
        let response = response.error_for_status()?;
        let payload = response
            .json::<TokenResponse>()
            .await
            .map_err(|error| OpenF1LiveError::TokenResponse(error.to_string()))?;
        if payload.access_token.trim().is_empty() {
            return Err(OpenF1LiveError::TokenResponse(
                "empty access_token".to_string(),
            ));
        }
        Ok(IssuedToken {
            access_token: payload.access_token,
            expires_at: Instant::now() + token_lifetime(payload.expires_in),
        })
    }

    fn ensure_valid_config(&self) -> Result<(), OpenF1LiveError> {
        if let Some(error) = &self.config_error {
            return Err(OpenF1LiveError::Config(error.clone()));
        }
        Ok(())
    }
}

fn auth_headers(
    config: &OpenF1LiveConfig,
    bearer: Option<&BearerToken>,
) -> Result<HeaderMap, OpenF1LiveError> {
    let mut headers = HeaderMap::new();
    let Some(bearer) = bearer else {
        return Ok(headers);
    };
    let name = HeaderName::from_bytes(config.auth_header.as_bytes())
        .map_err(|_| OpenF1LiveError::InvalidAuthHeader(config.auth_header.clone()))?;
    let value = if name == AUTHORIZATION {
        authorization_header_value(&bearer.value)
    } else {
        bearer.value.clone()
    };
    headers.insert(
        name,
        HeaderValue::from_str(&value).map_err(|_| OpenF1LiveError::InvalidAuthHeaderValue)?,
    );
    Ok(headers)
}

impl LiveRequestLimiter {
    fn new(interval: Duration, per_window: usize, window: Duration) -> Self {
        Self {
            interval,
            per_window,
            window,
            next_request: None,
            paused_until: None,
            sent: std::collections::VecDeque::new(),
        }
    }

    /// Blocks until a request may go out, then books it. Callers hold the limiter's
    /// mutex across this call, which is what serialises them.
    async fn wait_turn(&mut self) {
        loop {
            let now = Instant::now();
            while self
                .sent
                .front()
                .is_some_and(|sent| now.duration_since(*sent) >= self.window)
            {
                self.sent.pop_front();
            }
            let mut ready_at = now;
            if let Some(next) = self.next_request.filter(|next| *next > ready_at) {
                ready_at = next;
            }
            if let Some(paused) = self.paused_until.filter(|paused| *paused > ready_at) {
                ready_at = paused;
            }
            if self.sent.len() >= self.per_window {
                if let Some(oldest) = self.sent.front() {
                    ready_at = ready_at.max(*oldest + self.window);
                }
            }
            if ready_at <= now {
                break;
            }
            tokio::time::sleep(ready_at.duration_since(now)).await;
        }
        let now = Instant::now();
        self.sent.push_back(now);
        self.next_request = Some(now + self.interval);
    }

    /// Holds every request back until `until`. Called on a 429 so the other pollers
    /// stop piling onto a budget that is already spent.
    fn pause_until(&mut self, until: Instant) {
        if self.paused_until.is_none_or(|paused| until > paused) {
            self.paused_until = Some(until);
        }
    }
}

fn live_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(OPENF1_LIVE_REQUEST_TIMEOUT_SECONDS))
        .build()
        .expect("valid OpenF1 live HTTP client")
}

/// Builds a live endpoint URL. OpenF1's comparison filters put the operator in the key
/// (`date>=2026-...`) and the server parses the percent-decoded query as a whole, so the
/// decoded string must read `date>=VALUE`. `query_pairs_mut` would send `date%3E%3D=VALUE`,
/// which decodes to `date>==VALUE`, an unknown filter that matches nothing and comes back
/// 404. Here a trailing `=` in the key becomes the key/value separator itself (the `url`
/// crate still escapes `>`, giving `date%3E=VALUE`, which is verified to work).
fn live_endpoint_url(
    base_url: &Url,
    endpoint: &str,
    params: &[(String, String)],
) -> Result<Url, OpenF1LiveError> {
    let mut url = base_url.join(endpoint)?;
    let query = params
        .iter()
        .map(|(key, value)| {
            let key = key.strip_suffix('=').unwrap_or(key);
            let value: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
            format!("{key}={value}")
        })
        .collect::<Vec<_>>()
        .join("&");
    url.set_query(Some(&query));
    Ok(url)
}

/// A `Retry-After` header given in seconds. The HTTP-date form is not handled; the
/// default pause covers it.
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// `{"detail":"No results found."}`, OpenF1's body for an empty result set.
fn is_no_results_body(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("detail")?.as_str().map(str::to_ascii_lowercase))
        .is_some_and(|detail| detail.contains("no results"))
}

fn is_auth_rejection(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
}

/// The token endpoint lives at the API host root, not under `/v1/`, so it is derived
/// from the configured base URL rather than joined onto it. A compatible mirror at
/// `https://example.test/v1/` is expected to issue tokens at `https://example.test/token`.
fn token_url(base_url: &Url) -> Result<Url, OpenF1LiveError> {
    Ok(base_url.join(TOKEN_PATH)?)
}

fn token_lifetime(expires_in: Option<u64>) -> Duration {
    let seconds = expires_in.unwrap_or(DEFAULT_TOKEN_LIFETIME_SECONDS);
    Duration::from_secs(seconds.saturating_sub(TOKEN_REFRESH_MARGIN_SECONDS))
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
    #[error("OpenF1 rejected the credentials (HTTP {0})")]
    Unauthorized(u16),
    #[error("OpenF1 token endpoint returned an unexpected response: {0}")]
    TokenResponse(String),
    #[error("OpenF1 rate limit reached on {endpoint} (HTTP 429); live polling paused for {pause_seconds}s")]
    RateLimited { endpoint: String, pause_seconds: u64 },
    #[error("OpenF1 {endpoint} returned HTTP {status}: {body}")]
    Status {
        endpoint: String,
        status: u16,
        body: String,
    },
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
                auth: OpenF1Auth::None,
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
            auth: OpenF1Auth::None,
            auth_header: AUTHORIZATION.as_str().to_string(),
        });

        assert_eq!(
            client.config().base_url.join("sessions").unwrap().as_str(),
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
    fn live_endpoint_url_keeps_filter_operators_unencoded() {
        let since = DateTime::parse_from_rfc3339("2026-09-06T13:50:47.933Z")
            .unwrap()
            .with_timezone(&Utc);
        let url = live_endpoint_url(
            &Url::parse(DEFAULT_BASE_URL).unwrap(),
            "intervals",
            &live_endpoint_params(11361, "intervals", Some(since)),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.openf1.org/v1/intervals?session_key=11361&date%3E=2026-09-06T13%3A50%3A47.933%2B00%3A00"
        );
        assert_eq!(
            percent_encoding_free(url.query().unwrap()),
            "session_key=11361&date>=2026-09-06T13:50:47.933+00:00"
        );
    }

    /// What OpenF1 sees after decoding the query string.
    fn percent_encoding_free(query: &str) -> String {
        url::form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    #[test]
    fn no_results_body_is_recognised() {
        assert!(is_no_results_body(r#"{"detail":"No results found."}"#));
        assert!(!is_no_results_body(r#"{"detail":"Not Found"}"#));
        assert!(!is_no_results_body(""));
        assert!(!is_no_results_body("<html>404</html>"));
    }

    #[test]
    fn only_location_is_windowed_on_the_first_fetch_and_only_with_a_date_field() {
        for spec in LIVE_ENDPOINTS {
            if spec.name == "location" {
                assert_eq!(spec.initial_window_seconds, Some(240));
            } else {
                assert_eq!(spec.initial_window_seconds, None, "{}", spec.name);
            }
            if spec.initial_window_seconds.is_some() {
                assert!(spec.incremental_field.is_some(), "{} cannot be windowed", spec.name);
            }
        }
        let now = DateTime::parse_from_rfc3339("2026-09-06T14:40:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let location = LIVE_ENDPOINTS.iter().find(|spec| spec.name == "location").unwrap();
        assert_eq!(
            location.initial_since(now).unwrap().to_rfc3339(),
            "2026-09-06T14:36:00+00:00"
        );
    }

    #[test]
    fn live_endpoint_cadences_stay_under_the_openf1_rate_limit() {
        let per_minute: f64 = LIVE_ENDPOINTS
            .iter()
            .map(|spec| 60_000.0 / spec.cadence_ms as f64)
            .sum();
        assert!(
            per_minute <= OPENF1_LIVE_REQUESTS_PER_MINUTE as f64 - 2.0,
            "{per_minute} requests/min leaves no room under the {OPENF1_LIVE_REQUESTS_PER_MINUTE}/min budget"
        );
        assert!(OPENF1_LIVE_REQUESTS_PER_MINUTE <= 60);
        let limiter_per_second = 1_000.0 / OPENF1_LIVE_REQUEST_INTERVAL_MS as f64;
        assert!(limiter_per_second < 6.0, "{limiter_per_second} requests/s");
    }

    #[tokio::test]
    async fn request_limiter_enforces_the_per_window_budget() {
        let window = Duration::from_millis(300);
        let mut limiter = LiveRequestLimiter::new(Duration::ZERO, 3, window);
        let started = Instant::now();
        for _ in 0..3 {
            limiter.wait_turn().await;
        }
        assert!(started.elapsed() < Duration::from_millis(100), "budget should be free");
        limiter.wait_turn().await;
        assert!(
            started.elapsed() >= window,
            "fourth request should wait for the window: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn request_limiter_pause_holds_every_request() {
        let mut limiter = LiveRequestLimiter::new(Duration::ZERO, usize::MAX, Duration::from_secs(60));
        let pause = Duration::from_millis(200);
        limiter.pause_until(Instant::now() + pause);
        let started = Instant::now();
        limiter.wait_turn().await;
        assert!(started.elapsed() >= pause, "{:?}", started.elapsed());
    }

    #[test]
    fn retry_after_reads_seconds_only() {
        let mut headers = HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("7"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));
        headers.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
        );
        assert_eq!(retry_after(&headers), None);
        assert_eq!(retry_after(&HeaderMap::new()), None);
    }

    #[test]
    fn live_endpoint_cadence_seconds_uses_endpoint_specs() {
        assert_eq!(live_endpoint_cadence_seconds("location"), 5.0);
        assert_eq!(live_endpoint_cadence_seconds("weather"), 60.0);
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

    #[test]
    fn token_url_is_at_the_api_host_root() {
        assert_eq!(
            token_url(&Url::parse(DEFAULT_BASE_URL).unwrap())
                .unwrap()
                .as_str(),
            "https://api.openf1.org/token"
        );
        assert_eq!(
            token_url(&Url::parse("https://example.test/v1/").unwrap())
                .unwrap()
                .as_str(),
            "https://example.test/token"
        );
    }

    #[test]
    fn token_response_accepts_expires_in_as_string_or_number() {
        let text: TokenResponse = serde_json::from_str(
            r#"{"expires_in":"3600","access_token":"t","token_type":"bearer"}"#,
        )
        .unwrap();
        assert_eq!(text.expires_in, Some(3_600));
        let number: TokenResponse =
            serde_json::from_str(r#"{"expires_in":3600,"access_token":"t"}"#).unwrap();
        assert_eq!(number.expires_in, Some(3_600));
        let unparseable: TokenResponse =
            serde_json::from_str(r#"{"expires_in":"soon","access_token":"t"}"#).unwrap();
        assert_eq!(unparseable.expires_in, None);
        let missing: TokenResponse = serde_json::from_str(r#"{"access_token":"t"}"#).unwrap();
        assert_eq!(missing.expires_in, None);
    }

    #[test]
    fn token_lifetime_keeps_a_refresh_margin() {
        assert_eq!(token_lifetime(Some(3_600)), Duration::from_secs(3_480));
        assert_eq!(token_lifetime(None), Duration::from_secs(3_480));
        assert_eq!(token_lifetime(Some(30)), Duration::ZERO);
    }

    /// A stand-in for api.openf1.org: issues numbered tokens at `/token` for one
    /// accepted login and answers `/v1/sessions` only for tokens it still honours.
    mod mock {
        use axum::{
            extract::State,
            http::{HeaderMap, StatusCode},
            routing::{get, post},
            Form, Json, Router,
        };
        use serde::Deserialize;
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        pub struct Recorded {
            pub exchanges: usize,
            pub revoked: Vec<String>,
            pub seen_authorization: Vec<String>,
        }

        #[derive(Clone)]
        struct MockState {
            recorded: Arc<Mutex<Recorded>>,
            expires_in: Option<u64>,
        }

        #[derive(Deserialize)]
        struct LoginForm {
            username: String,
            password: String,
        }

        pub struct Server {
            pub base_url: reqwest::Url,
            pub recorded: Arc<Mutex<Recorded>>,
        }

        pub async fn spawn(expires_in: Option<u64>) -> Server {
            let recorded = Arc::new(Mutex::new(Recorded::default()));
            let app = Router::new()
                .route("/token", post(token))
                .route("/v1/sessions", get(sessions))
                .route("/v1/session_result", get(no_results))
                .route("/v1/laps", get(too_many_requests))
                .with_state(MockState {
                    recorded: Arc::clone(&recorded),
                    expires_in,
                });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            Server {
                base_url: format!("http://{addr}/v1/").parse().unwrap(),
                recorded,
            }
        }

        async fn token(
            State(state): State<MockState>,
            Form(form): Form<LoginForm>,
        ) -> Result<Json<serde_json::Value>, StatusCode> {
            if form.username != "driver@example.com" || form.password != "correct horse" {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let mut recorded = state.recorded.lock().unwrap();
            recorded.exchanges += 1;
            let mut body = serde_json::json!({
                "access_token": format!("issued-{}", recorded.exchanges),
                "token_type": "bearer",
            });
            if let Some(expires_in) = state.expires_in {
                // OpenF1 sends this as a string, not a number.
                body["expires_in"] = serde_json::json!(expires_in.to_string());
            }
            Ok(Json(body))
        }

        async fn too_many_requests() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "3")], "")
        }

        async fn no_results() -> (StatusCode, Json<serde_json::Value>) {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "detail": "No results found." })),
            )
        }

        async fn sessions(
            State(state): State<MockState>,
            headers: HeaderMap,
        ) -> Result<Json<serde_json::Value>, StatusCode> {
            let authorization = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let mut recorded = state.recorded.lock().unwrap();
            recorded.seen_authorization.push(authorization.clone());
            let token = authorization.strip_prefix("Bearer ").unwrap_or_default();
            if !token.starts_with("issued-") || recorded.revoked.iter().any(|r| r == token) {
                return Err(StatusCode::UNAUTHORIZED);
            }
            Ok(Json(serde_json::json!([])))
        }
    }

    fn login_client(base_url: Url) -> OpenF1LiveClient {
        OpenF1LiveClient::with_config(OpenF1LiveConfig {
            enabled: true,
            base_url,
            auth: OpenF1Auth::Login(OpenF1Login {
                username: "driver@example.com".to_string(),
                password: "correct horse".to_string(),
            }),
            auth_header: AUTHORIZATION.as_str().to_string(),
        })
    }

    #[tokio::test]
    async fn login_is_exchanged_once_and_the_token_reused() {
        let server = mock::spawn(Some(3_600)).await;
        let client = login_client(server.base_url.clone());

        client.probe().await.unwrap();
        client.probe().await.unwrap();

        let recorded = server.recorded.lock().unwrap();
        assert_eq!(recorded.exchanges, 1);
        assert_eq!(
            recorded.seen_authorization,
            vec!["Bearer issued-1".to_string(), "Bearer issued-1".to_string()]
        );
    }

    #[tokio::test]
    async fn expired_token_is_exchanged_again_before_the_next_request() {
        // `expires_in` at or under the refresh margin leaves no usable lifetime, so the
        // second request must go back to the token endpoint.
        let server = mock::spawn(Some(60)).await;
        let client = login_client(server.base_url.clone());

        client.probe().await.unwrap();
        client.probe().await.unwrap();

        let recorded = server.recorded.lock().unwrap();
        assert_eq!(recorded.exchanges, 2);
        assert_eq!(recorded.seen_authorization.last().unwrap(), "Bearer issued-2");
    }

    #[tokio::test]
    async fn revoked_cached_token_is_exchanged_again_and_the_request_retried_once() {
        let server = mock::spawn(Some(3_600)).await;
        let client = login_client(server.base_url.clone());
        client.probe().await.unwrap();
        server
            .recorded
            .lock()
            .unwrap()
            .revoked
            .push("issued-1".to_string());

        client.probe().await.unwrap();

        let recorded = server.recorded.lock().unwrap();
        assert_eq!(recorded.exchanges, 2);
        assert_eq!(
            recorded.seen_authorization,
            vec![
                "Bearer issued-1".to_string(),
                "Bearer issued-1".to_string(),
                "Bearer issued-2".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn a_fresh_token_that_is_rejected_is_not_exchanged_again() {
        let server = mock::spawn(Some(3_600)).await;
        let client = login_client(server.base_url.clone());
        // Revoke every token the mock will ever issue.
        {
            let mut recorded = server.recorded.lock().unwrap();
            recorded.revoked.push("issued-1".to_string());
            recorded.revoked.push("issued-2".to_string());
        }

        let error = client.probe().await.unwrap_err();

        assert!(matches!(error, OpenF1LiveError::Unauthorized(401)), "{error}");
        assert_eq!(server.recorded.lock().unwrap().exchanges, 1);
    }

    #[tokio::test]
    async fn a_rejected_login_reports_unauthorized() {
        let server = mock::spawn(Some(3_600)).await;
        let client = OpenF1LiveClient::with_config(OpenF1LiveConfig {
            enabled: true,
            base_url: server.base_url.clone(),
            auth: OpenF1Auth::Login(OpenF1Login {
                username: "driver@example.com".to_string(),
                password: "wrong".to_string(),
            }),
            auth_header: AUTHORIZATION.as_str().to_string(),
        });

        let error = client.probe().await.unwrap_err();

        assert!(matches!(error, OpenF1LiveError::Unauthorized(401)), "{error}");
        assert!(server.recorded.lock().unwrap().seen_authorization.is_empty());
    }

    #[tokio::test]
    async fn changing_the_login_forgets_the_issued_token() {
        let server = mock::spawn(Some(3_600)).await;
        let client = login_client(server.base_url.clone());
        client.probe().await.unwrap();

        client
            .set_auth(OpenF1Auth::Login(OpenF1Login {
                username: "driver@example.com".to_string(),
                password: "correct horse".to_string(),
            }))
            .await;
        client.probe().await.unwrap();

        assert_eq!(server.recorded.lock().unwrap().exchanges, 2);
    }

    #[tokio::test]
    async fn a_no_results_404_is_an_empty_payload_and_any_other_404_is_an_error() {
        let server = mock::spawn(Some(3_600)).await;
        let client = login_client(server.base_url.clone());

        let payload = client
            .fetch_live_bundle_endpoint(11361, "session_result")
            .await
            .unwrap()
            .payload;
        assert_eq!(payload, Value::Array(vec![]));

        // Axum's fallback answers an unknown route with an empty-bodied 404.
        let error = client
            .fetch_live_bundle_endpoint(11361, "nowhere")
            .await
            .unwrap_err();
        assert!(
            matches!(error, OpenF1LiveError::Status { status: 404, .. }),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_429_pauses_the_limiter_for_retry_after() {
        let server = mock::spawn(Some(3_600)).await;
        let client = login_client(server.base_url.clone());

        let error = client
            .fetch_live_bundle_endpoint(11361, "laps")
            .await
            .unwrap_err();
        assert!(
            matches!(&error, OpenF1LiveError::RateLimited { pause_seconds: 3, .. }),
            "{error}"
        );
        let paused_until = client.request_limiter.lock().await.paused_until.unwrap();
        let remaining = paused_until.saturating_duration_since(Instant::now());
        assert!(remaining > Duration::from_secs(2) && remaining <= Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_static_token_is_sent_without_an_exchange() {
        let server = mock::spawn(Some(3_600)).await;
        let client = OpenF1LiveClient::with_config(OpenF1LiveConfig {
            enabled: true,
            base_url: server.base_url.clone(),
            auth: OpenF1Auth::Token("issued-static".to_string()),
            auth_header: AUTHORIZATION.as_str().to_string(),
        });

        client.probe().await.unwrap();

        let recorded = server.recorded.lock().unwrap();
        assert_eq!(recorded.exchanges, 0);
        assert_eq!(recorded.seen_authorization, vec!["Bearer issued-static".to_string()]);
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
