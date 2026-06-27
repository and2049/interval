use crate::connectors::{
    fastf1_historical::FastF1HistoricalClient, openf1_historical::HistoricalClient,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use sqlx::SqlitePool;
use thiserror::Error;

mod discovery;
mod ingest;
mod replay_routes;
mod stream;

#[derive(Clone)]
pub struct AppState {
    pub(crate) pool: SqlitePool,
    pub(crate) historical: HistoricalClient,
    pub(crate) fastf1: FastF1HistoricalClient,
}

impl AppState {
    pub fn new(pool: SqlitePool, historical: HistoricalClient) -> Self {
        Self {
            pool,
            historical,
            fastf1: FastF1HistoricalClient::default(),
        }
    }

    #[cfg(test)]
    pub fn new_with_fastf1(
        pool: SqlitePool,
        historical: HistoricalClient,
        fastf1: FastF1HistoricalClient,
    ) -> Self {
        Self {
            pool,
            historical,
            fastf1,
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/seasons", get(discovery::seasons))
        .route("/api/meetings", get(discovery::meetings))
        .route("/api/sessions", get(discovery::sessions))
        .route(
            "/api/sessions/{session_key}/ingest",
            post(ingest::ingest_session),
        )
        .route(
            "/api/sessions/{session_key}/replay/metadata",
            get(replay_routes::replay_metadata),
        )
        .route(
            "/api/sessions/{session_key}/replay/snapshot",
            get(replay_routes::replay_snapshot),
        )
        .route(
            "/api/sessions/{session_key}/replay/events",
            get(replay_routes::replay_events),
        )
        .route(
            "/api/sessions/{session_key}/replay/stream",
            get(stream::replay_stream),
        )
        .route(
            "/api/sessions/{session_key}/track/geometry",
            get(replay_routes::track_geometry),
        )
        .with_state(state)
}

async fn healthz() -> Json<Health> {
    Json(Health { ok: true })
}

#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("resource not found")]
    NotFound,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("storage error: {0}")]
    Storage(#[from] anyhow::Error),
    #[error("historical connector error: {0}")]
    Historical(#[from] crate::connectors::openf1_historical::HistoricalError),
    #[error("FastF1 historical connector error: {0}")]
    FastF1Historical(#[from] crate::connectors::fastf1_historical::FastF1HistoricalError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Historical(_) | ApiError::FastF1Historical(_) => StatusCode::BAD_GATEWAY,
            ApiError::Database(_) | ApiError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = Json(serde_json::json!({
            "error": self.to_string()
        }));
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests;
