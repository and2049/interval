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
mod live;
mod live_simulation;
mod replay_routes;
mod stream;

#[derive(Clone)]
pub struct AppState {
    pub(crate) pool: SqlitePool,
    pub(crate) historical: HistoricalClient,
    pub(crate) fastf1: FastF1HistoricalClient,
    pub(crate) live: crate::live::OpenF1LiveRegistry,
    pub(crate) live_simulation: crate::live_simulation::LiveSimulationRegistry,
}

impl AppState {
    pub fn new(pool: SqlitePool, historical: HistoricalClient) -> Self {
        Self {
            pool,
            historical,
            fastf1: FastF1HistoricalClient::default(),
            live: crate::live::OpenF1LiveRegistry::new(
                crate::connectors::openf1_live::OpenF1LiveClient::default(),
            ),
            live_simulation: crate::live_simulation::LiveSimulationRegistry::default(),
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
            live: crate::live::OpenF1LiveRegistry::new(
                crate::connectors::openf1_live::OpenF1LiveClient::default(),
            ),
            live_simulation: crate::live_simulation::LiveSimulationRegistry::default(),
        }
    }

    #[cfg(test)]
    pub fn new_with_live(
        pool: SqlitePool,
        historical: HistoricalClient,
        live_client: crate::connectors::openf1_live::OpenF1LiveClient,
    ) -> Self {
        Self {
            pool,
            historical,
            fastf1: FastF1HistoricalClient::default(),
            live: crate::live::OpenF1LiveRegistry::new(live_client),
            live_simulation: crate::live_simulation::LiveSimulationRegistry::default(),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/seasons", get(discovery::seasons))
        .route("/api/meetings", get(discovery::meetings))
        .route("/api/sessions", get(discovery::sessions))
        .route("/api/live/current", get(live::current))
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
        .route("/api/sessions/{session_key}/live/start", post(live::start))
        .route("/api/sessions/{session_key}/live/status", get(live::status))
        .route(
            "/api/sessions/{session_key}/live/metadata",
            get(live::metadata),
        )
        .route(
            "/api/sessions/{session_key}/live/snapshot",
            get(live::snapshot),
        )
        .route("/api/sessions/{session_key}/live/stream", get(live::stream))
        .route(
            "/api/sessions/{session_key}/live/track/geometry",
            get(live::track_geometry),
        )
        .route("/api/sessions/{session_key}/live/events", get(live::events))
        .route("/api/sessions/{session_key}/live/stop", post(live::stop))
        .route(
            "/api/sessions/{session_key}/live-simulation/start",
            post(live_simulation::start),
        )
        .route(
            "/api/sessions/{session_key}/live-simulation/status",
            get(live_simulation::status),
        )
        .route(
            "/api/sessions/{session_key}/live-simulation/snapshot",
            get(live_simulation::snapshot),
        )
        .route(
            "/api/sessions/{session_key}/live-simulation/stream",
            get(live_simulation::stream),
        )
        .route(
            "/api/sessions/{session_key}/live-simulation/stop",
            post(live_simulation::stop),
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
    #[error("bad gateway: {0}")]
    BadGateway(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
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
            ApiError::BadGateway(_) => StatusCode::BAD_GATEWAY,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
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
