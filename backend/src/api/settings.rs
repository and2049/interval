//! Desktop-only settings routes.
//!
//! These are registered by `main.rs` only when `INTERVAL_ENABLE_SETTINGS_API` is set,
//! and are merged *after* the permissive CORS layer so they carry no
//! `Access-Control-Allow-Origin` and no preflight handler. There is no authentication
//! anywhere in this service, so a browser on some other origin must not be able to read
//! or overwrite the user's API token.
//!
//! No response here ever contains the token itself, only a masked hint.

use super::{ApiError, AppState};
use crate::connectors::openf1_live::OpenF1LiveError;
use crate::settings::{resolve_openf1_token, token_hint};
use axum::{extract::State, Json};
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct OpenF1TokenSettings {
    configured: bool,
    /// Masked fingerprint, never the token.
    hint: Option<String>,
    /// "settings" | "env" | "none"
    source: &'static str,
    /// True when INTERVAL_OPENF1_LIVE_TOKEN is also set, so the UI can explain which
    /// value is actually in use instead of leaving the user to guess.
    env_token_present: bool,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenF1TokenProbe {
    /// "ok" | "unauthorized" | "unreachable" | "invalid"
    result: &'static str,
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct SetTokenRequest {
    token: String,
}

pub async fn get_token(State(state): State<AppState>) -> Json<OpenF1TokenSettings> {
    Json(current_settings(&state))
}

// `Json` consumes the request body, so it must be the last extractor.
pub async fn put_token(
    State(state): State<AppState>,
    Json(body): Json<SetTokenRequest>,
) -> Result<Json<OpenF1TokenSettings>, ApiError> {
    let token = body.token.trim();
    if token.is_empty() {
        return Err(ApiError::BadRequest("token must not be blank".to_string()));
    }
    // Fail now rather than on every OpenF1 request later.
    if HeaderValue::from_str(token).is_err() {
        return Err(ApiError::BadRequest(
            "token contains characters that cannot be sent in an HTTP header".to_string(),
        ));
    }

    // Persist first: a write failure must not leave a token applied in memory that
    // silently disappears on the next restart.
    state.settings.store_token(Some(token))?;
    state
        .live
        .apply_openf1_token(Some(token.to_string()))
        .await;

    Ok(Json(current_settings(&state)))
}

pub async fn delete_token(
    State(state): State<AppState>,
) -> Result<Json<OpenF1TokenSettings>, ApiError> {
    state.settings.store_token(None)?;
    // Fall back to the environment rather than clearing the token outright.
    let (token, _source) = resolve_openf1_token(None, env_token());
    state.live.apply_openf1_token(token).await;
    Ok(Json(current_settings(&state)))
}

/// Always returns 200: a rejected token is a successful diagnosis, not a failed request.
pub async fn test_token(State(state): State<AppState>) -> Json<OpenF1TokenProbe> {
    let (result, message) = match state.live.probe_openf1().await {
        Ok(()) => ("ok", "OpenF1 accepted the token.".to_string()),
        Err(OpenF1LiveError::Unauthorized(status)) => (
            "unauthorized",
            format!("OpenF1 rejected the token (HTTP {status})."),
        ),
        Err(OpenF1LiveError::Request(error)) if error.is_timeout() => (
            "unreachable",
            "OpenF1 did not respond in time.".to_string(),
        ),
        Err(OpenF1LiveError::Request(error)) => {
            ("unreachable", format!("Could not reach OpenF1: {error}"))
        }
        Err(OpenF1LiveError::Normalize(_)) => (
            "invalid",
            "OpenF1 returned an unexpected response.".to_string(),
        ),
        Err(error) => ("invalid", error.to_string()),
    };
    Json(OpenF1TokenProbe {
        result,
        message: message.to_string(),
    })
}

fn current_settings(state: &AppState) -> OpenF1TokenSettings {
    let stored = state.settings.load().openf1_token;
    let env = env_token();
    let env_token_present = env
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let (token, source) = resolve_openf1_token(stored, env);
    OpenF1TokenSettings {
        configured: token.is_some(),
        hint: token.as_deref().map(token_hint),
        source: source.as_str(),
        env_token_present,
        path: state.settings.path_display(),
    }
}

fn env_token() -> Option<String> {
    std::env::var("INTERVAL_OPENF1_LIVE_TOKEN").ok()
}
