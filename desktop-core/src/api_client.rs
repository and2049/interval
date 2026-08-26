//! HTTP client for the embedded backend, mirroring `frontend/src/lib/api.ts`.
//!
//! One method per endpoint, deserializing straight into `interval_backend::domain`
//! types — the wire contract is the backend's serde output, so there is nothing to
//! mirror by hand. Every request must run on a tokio runtime (reqwest needs the
//! reactor); the GPUI shell spawns these on its captured runtime handle.

use interval_backend::domain::{
    IngestResponse, LiveCurrentResponse, LiveSessionStatus, Meeting, ReplayEventListResponse,
    ReplayMetadata, ReplaySnapshot, Season, SessionReadiness, TrackGeometry,
};

use crate::settings_panel::{OpenF1LoginProbe, OpenF1LoginSettings};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// A non-2xx response, carrying the server's `error`/`message` body field when one
    /// exists, the trimmed body otherwise, or "<status> <reason>" as the fallback —
    /// the same precedence as the frontend's `apiErrorMessage`.
    #[error("{message}")]
    Status { status: u16, message: String },
    #[error("{0}")]
    Transport(#[from] reqwest::Error),
    #[error("{0}")]
    Decode(#[from] serde_json::Error),
}

impl ApiError {
    /// The 404 the settings routes return in non-desktop deployments, which the UI
    /// uses to hide the settings gear rather than report an error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, ApiError::Status { status: 404, .. })
    }
}

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
}

impl ApiClient {
    /// `base_url` without a trailing slash, e.g. `http://127.0.0.1:45123`.
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let response = self.http.get(self.url(path)).send().await?;
        decode_json(response).await
    }

    async fn post_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let response = self.http.post(self.url(path)).send().await?;
        decode_json(response).await
    }

    pub async fn seasons(&self) -> Result<Vec<Season>, ApiError> {
        self.get_json("/api/seasons").await
    }

    pub async fn meetings(&self, season: i32) -> Result<Vec<Meeting>, ApiError> {
        self.get_json(&format!("/api/meetings?season={season}")).await
    }

    pub async fn sessions(&self, meeting_key: i64) -> Result<Vec<SessionReadiness>, ApiError> {
        self.get_json(&format!("/api/sessions?meeting_key={meeting_key}"))
            .await
    }

    /// Mirrors the frontend's `ingestResponseOrThrow`: the ingest route reports
    /// failures as a well-formed `IngestResponse` on error statuses too, so a body
    /// that parses as one is returned regardless of status.
    pub async fn ingest(&self, session_key: i64) -> Result<IngestResponse, ApiError> {
        let response = self
            .http
            .post(self.url(&format!("/api/sessions/{session_key}/ingest")))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if let Ok(parsed) = serde_json::from_str::<IngestResponse>(&body) {
            return Ok(parsed);
        }
        if !status.is_success() {
            return Err(status_error(status, &body));
        }
        Err(ApiError::Status {
            status: status.as_u16(),
            message: "Invalid ingest response.".to_string(),
        })
    }

    pub async fn metadata(&self, session_key: i64) -> Result<ReplayMetadata, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/replay/metadata"))
            .await
    }

    pub async fn snapshot(&self, session_key: i64, t: f64) -> Result<ReplaySnapshot, ApiError> {
        self.get_json(&format!(
            "/api/sessions/{session_key}/replay/snapshot?t={t:.3}"
        ))
        .await
    }

    pub fn stream_url(&self, session_key: i64, from: f64, speed: f64) -> String {
        self.url(&format!(
            "/api/sessions/{session_key}/replay/stream?from={from:.3}&speed={speed:.3}"
        ))
    }

    pub async fn events(&self, session_key: i64) -> Result<ReplayEventListResponse, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/replay/events"))
            .await
    }

    pub async fn track_geometry(&self, session_key: i64) -> Result<TrackGeometry, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/track/geometry"))
            .await
    }

    pub async fn live_current(&self) -> Result<LiveCurrentResponse, ApiError> {
        self.get_json("/api/live/current").await
    }

    pub async fn live_start(&self, session_key: i64) -> Result<LiveSessionStatus, ApiError> {
        self.post_json(&format!("/api/sessions/{session_key}/live/start"))
            .await
    }

    pub async fn live_status(&self, session_key: i64) -> Result<LiveSessionStatus, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/live/status"))
            .await
    }

    pub async fn live_metadata(&self, session_key: i64) -> Result<ReplayMetadata, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/live/metadata"))
            .await
    }

    pub async fn live_snapshot(&self, session_key: i64) -> Result<ReplaySnapshot, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/live/snapshot"))
            .await
    }

    pub fn live_stream_url(&self, session_key: i64) -> String {
        self.url(&format!("/api/sessions/{session_key}/live/stream"))
    }

    pub async fn live_events(&self, session_key: i64) -> Result<ReplayEventListResponse, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/live/events"))
            .await
    }

    pub async fn live_track_geometry(&self, session_key: i64) -> Result<TrackGeometry, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/live/track/geometry"))
            .await
    }

    pub async fn live_stop(&self, session_key: i64) -> Result<LiveSessionStatus, ApiError> {
        self.post_json(&format!("/api/sessions/{session_key}/live/stop"))
            .await
    }

    pub async fn live_simulation_start(
        &self,
        session_key: i64,
    ) -> Result<LiveSessionStatus, ApiError> {
        self.post_json(&format!("/api/sessions/{session_key}/live-simulation/start"))
            .await
    }

    pub async fn live_simulation_status(
        &self,
        session_key: i64,
    ) -> Result<LiveSessionStatus, ApiError> {
        self.get_json(&format!("/api/sessions/{session_key}/live-simulation/status"))
            .await
    }

    pub async fn live_simulation_snapshot(
        &self,
        session_key: i64,
    ) -> Result<ReplaySnapshot, ApiError> {
        self.get_json(&format!(
            "/api/sessions/{session_key}/live-simulation/snapshot"
        ))
        .await
    }

    pub fn live_simulation_stream_url(&self, session_key: i64) -> String {
        self.url(&format!("/api/sessions/{session_key}/live-simulation/stream"))
    }

    pub async fn live_simulation_stop(
        &self,
        session_key: i64,
    ) -> Result<LiveSessionStatus, ApiError> {
        self.post_json(&format!("/api/sessions/{session_key}/live-simulation/stop"))
            .await
    }

    // Settings routes exist only when the embedded server enables them; in other
    // deployments they 404, which is how the UI decides not to show the gear.

    pub async fn openf1_login(&self) -> Result<OpenF1LoginSettings, ApiError> {
        self.get_json(OPENF1_LOGIN_PATH).await
    }

    pub async fn save_openf1_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<OpenF1LoginSettings, ApiError> {
        let response = self
            .http
            .put(self.url(OPENF1_LOGIN_PATH))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await?;
        decode_json(response).await
    }

    pub async fn clear_openf1_login(&self) -> Result<OpenF1LoginSettings, ApiError> {
        let response = self.http.delete(self.url(OPENF1_LOGIN_PATH)).send().await?;
        decode_json(response).await
    }

    pub async fn test_openf1_login(&self) -> Result<OpenF1LoginProbe, ApiError> {
        self.post_json(&format!("{OPENF1_LOGIN_PATH}/test")).await
    }

    /// Open one of the `*_url` endpoints as a byte stream for [`crate::sse::SseParser`].
    /// The stream ending (Ok or Err) is the end-of-stream signal — the server's `end`
    /// event carries no data and is deliberately not dispatched, matching browsers.
    pub async fn open_stream(&self, url: &str) -> Result<reqwest::Response, ApiError> {
        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(status_error(status, &body));
        }
        Ok(response)
    }
}

const OPENF1_LOGIN_PATH: &str = "/api/settings/openf1-login";

async fn decode_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(status_error(status, &body));
    }
    let body = response.text().await?;
    Ok(serde_json::from_str(&body)?)
}

/// Same precedence as the frontend's `apiErrorMessageFromBody`: a string `error` or
/// `message` field wins, then the trimmed body, then "<status> <reason>".
fn status_error(status: reqwest::StatusCode, body: &str) -> ApiError {
    let fallback = format!(
        "{} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
    .trim()
    .to_string();
    let message = if body.trim().is_empty() {
        fallback
    } else {
        match serde_json::from_str::<serde_json::Value>(body) {
            Ok(payload) => {
                let field = |name: &str| {
                    payload
                        .get(name)
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| value.to_string())
                };
                field("error").or_else(|| field("message")).unwrap_or(fallback)
            }
            Err(_) => body.trim().to_string(),
        }
    };
    ApiError::Status {
        status: status.as_u16(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> reqwest::StatusCode {
        reqwest::StatusCode::from_u16(code).unwrap()
    }

    fn message_of(error: ApiError) -> String {
        match error {
            ApiError::Status { message, .. } => message,
            other => panic!("expected status error, got {other:?}"),
        }
    }

    #[test]
    fn error_prefers_the_error_field() {
        let error = status_error(status(500), r#"{"error":"boom","message":"nope"}"#);
        assert_eq!(message_of(error), "boom");
    }

    #[test]
    fn error_falls_back_to_the_message_field() {
        let error = status_error(status(500), r#"{"message":"try later"}"#);
        assert_eq!(message_of(error), "try later");
    }

    #[test]
    fn error_uses_the_raw_body_when_not_json() {
        let error = status_error(status(502), "  upstream broke  ");
        assert_eq!(message_of(error), "upstream broke");
    }

    #[test]
    fn error_uses_status_line_when_body_is_blank() {
        let error = status_error(status(503), "   ");
        assert_eq!(message_of(error), "503 Service Unavailable");
    }

    #[test]
    fn error_uses_status_line_when_json_fields_are_blank() {
        let error = status_error(status(404), r#"{"error":"  "}"#);
        assert_eq!(message_of(error), "404 Not Found");
    }

    #[test]
    fn not_found_detection() {
        assert!(status_error(status(404), "").is_not_found());
        assert!(!status_error(status(500), "").is_not_found());
    }

    #[test]
    fn stream_url_formats_times_to_three_decimals() {
        let client = ApiClient::new("http://127.0.0.1:4000/");
        assert_eq!(
            client.stream_url(9472, 12.0, 1.5),
            "http://127.0.0.1:4000/api/sessions/9472/replay/stream?from=12.000&speed=1.500"
        );
    }
}
