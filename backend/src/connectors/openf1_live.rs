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

