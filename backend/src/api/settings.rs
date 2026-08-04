//! Desktop-only settings routes.
//!
//! These are registered by `main.rs` only when `INTERVAL_ENABLE_SETTINGS_API` is set,
//! and are merged *after* the permissive CORS layer so they carry no
//! `Access-Control-Allow-Origin` and no preflight handler. There is no authentication
//! anywhere in this service, so a browser on some other origin must not be able to read
//! or overwrite the user's OpenF1 login.
//!
//! No response here ever contains the password. The username is an email address the
//! user typed in themselves and is echoed back so the panel can show who is signed in.

use super::{ApiError, AppState};
use crate::connectors::openf1_live::{OpenF1Auth, OpenF1Login, OpenF1LiveError};
use crate::settings::resolve_openf1_auth;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct OpenF1LoginSettings {
    configured: bool,
    /// The saved account's username. `None` when nothing is saved or the environment
    /// token is in use.
    username: Option<String>,
    /// "settings" | "env" | "none"
    source: &'static str,
    /// True when INTERVAL_OPENF1_LIVE_TOKEN is also set, so the UI can explain which
    /// value is actually in use instead of leaving the user to guess.
    env_token_present: bool,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenF1LoginProbe {
    /// "ok" | "unauthorized" | "unreachable" | "invalid"
    result: &'static str,
    message: String,
}

#[derive(Deserialize)]
pub struct SetLoginRequest {
    username: String,
    password: String,
}

pub async fn get_login(State(state): State<AppState>) -> Json<OpenF1LoginSettings> {
    Json(current_settings(&state))
}

// `Json` consumes the request body, so it must be the last extractor.
pub async fn put_login(
    State(state): State<AppState>,
    Json(body): Json<SetLoginRequest>,
) -> Result<Json<OpenF1LoginSettings>, ApiError> {
    let username = body.username.trim();
    if username.is_empty() {
        return Err(ApiError::BadRequest("username must not be blank".to_string()));
    }
    // Not trimmed: OpenF1 generates the password, and a generated password may well
    // start or end with whitespace. Only an all-blank one is rejected.
    if body.password.trim().is_empty() {
        return Err(ApiError::BadRequest("password must not be blank".to_string()));
    }
    let login = OpenF1Login {
        username: username.to_string(),
        password: body.password,
    };

    // Persist first: a write failure must not leave a login applied in memory that
    // silently disappears on the next restart.
    state.settings.store_login(Some(&login))?;
    state.live.apply_openf1_auth(OpenF1Auth::Login(login)).await;

    Ok(Json(current_settings(&state)))
}

pub async fn delete_login(
    State(state): State<AppState>,
) -> Result<Json<OpenF1LoginSettings>, ApiError> {
    state.settings.store_login(None)?;
    // Fall back to the environment rather than clearing the credentials outright.
    let (auth, _source) = resolve_openf1_auth(&Default::default(), env_token());
    state.live.apply_openf1_auth(auth).await;
    Ok(Json(current_settings(&state)))
}

/// Always returns 200: a rejected login is a successful diagnosis, not a failed request.
pub async fn test_login(State(state): State<AppState>) -> Json<OpenF1LoginProbe> {
    let (result, message) = match state.live.probe_openf1().await {
        Ok(()) => ("ok", "OpenF1 accepted the login.".to_string()),
        Err(OpenF1LiveError::Unauthorized(status)) => (
            "unauthorized",
            format!("OpenF1 rejected the login (HTTP {status})."),
        ),
        Err(OpenF1LiveError::Request(error)) if error.is_timeout() => (
            "unreachable",
            "OpenF1 did not respond in time.".to_string(),
        ),
        Err(OpenF1LiveError::Request(error)) => {
            ("unreachable", format!("Could not reach OpenF1: {error}"))
        }
        Err(OpenF1LiveError::Normalize(_)) | Err(OpenF1LiveError::TokenResponse(_)) => (
            "invalid",
            "OpenF1 returned an unexpected response.".to_string(),
        ),
        Err(error) => ("invalid", error.to_string()),
    };
    Json(OpenF1LoginProbe {
        result,
        message: message.to_string(),
    })
}

fn current_settings(state: &AppState) -> OpenF1LoginSettings {
    let stored = state.settings.load();
    let env = env_token();
    let env_token_present = env
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let (auth, source) = resolve_openf1_auth(&stored, env);
    let username = match &auth {
        OpenF1Auth::Login(login) => Some(login.username.clone()),
        OpenF1Auth::Token(_) | OpenF1Auth::None => None,
    };
    OpenF1LoginSettings {
        configured: auth.is_configured(),
        username,
        source: source.as_str(),
        env_token_present,
        path: state.settings.path_display(),
    }
}

fn env_token() -> Option<String> {
    std::env::var("INTERVAL_OPENF1_LIVE_TOKEN").ok()
}
