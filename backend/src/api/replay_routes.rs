use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::{replay, storage};

use super::{ApiError, AppState};

pub async fn track_geometry(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::TrackGeometry>, ApiError> {
    storage::get_track_geometry(&state.pool, session_key)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn replay_metadata(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::ReplayMetadata>, ApiError> {
    replay::metadata(&state.pool, session_key)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn replay_snapshot(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
    Query(query): Query<SnapshotQuery>,
) -> Result<Json<crate::domain::ReplaySnapshot>, ApiError> {
    replay::snapshot_at(&state.pool, session_key, query.replay_time()?)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn replay_events(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::ReplayEventListResponse>, ApiError> {
    if replay::metadata(&state.pool, session_key).await?.is_none() {
        return Err(ApiError::NotFound);
    }

    Ok(Json(crate::domain::ReplayEventListResponse {
        contract_version: crate::domain::REPLAY_CONTRACT_VERSION.to_string(),
        events: storage::get_replay_events(&state.pool, session_key).await?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SnapshotQuery {
    t: f64,
}

impl SnapshotQuery {
    fn replay_time(&self) -> Result<f64, ApiError> {
        if self.t.is_finite() {
            Ok(self.t)
        } else {
            Err(ApiError::BadRequest(
                "snapshot query parameter 't' must be a finite number".to_string(),
            ))
        }
    }
}
