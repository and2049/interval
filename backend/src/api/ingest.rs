use super::{ApiError, AppState};
use crate::{
    domain::{IngestResponse, IngestStatus},
    replay, storage,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::time::Duration;

pub async fn ingest_session(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Response, ApiError> {
    storage::get_session(&state.pool, session_key)
        .await?
        .ok_or(ApiError::NotFound)?;
    storage::set_ingest_status(
        &state.pool,
        session_key,
        crate::domain::IngestStatus::Fetching,
        None,
    )
    .await?;

    let bundle = match tokio::time::timeout(
        Duration::from_secs(90),
        state.historical.fetch_race_bundle(session_key),
    )
    .await
    {
        Ok(Ok(bundle)) => bundle,
        Ok(Err(error)) => {
            storage::set_ingest_status(
                &state.pool,
                session_key,
                IngestStatus::Failed,
                Some(&error.to_string()),
            )
            .await?;
            return Ok(failed_ingest_response(
                StatusCode::BAD_GATEWAY,
                session_key,
                0,
                error.to_string(),
            ));
        }
        Err(_) => {
            let message = "historical ingest timed out while fetching OpenF1 data";
            storage::set_ingest_status(
                &state.pool,
                session_key,
                IngestStatus::Failed,
                Some(message),
            )
            .await?;
            return Ok(failed_ingest_response(
                StatusCode::GATEWAY_TIMEOUT,
                session_key,
                0,
                message.to_string(),
            ));
        }
    };

    let cached_endpoints = bundle.len();
    storage::store_raw_bundle(&state.pool, &bundle).await?;
    storage::set_ingest_status(
        &state.pool,
        session_key,
        crate::domain::IngestStatus::Normalizing,
        None,
    )
    .await?;

    let built = match replay::rebuild_from_cache(&state.pool, session_key).await {
        Ok(built) => built,
        Err(error) => {
            storage::set_ingest_status(
                &state.pool,
                session_key,
                IngestStatus::Failed,
                Some(&error.to_string()),
            )
            .await?;
            return Ok(failed_ingest_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                session_key,
                cached_endpoints,
                error.to_string(),
            ));
        }
    };

    storage::set_ingest_status(
        &state.pool,
        session_key,
        crate::domain::IngestStatus::Ready,
        None,
    )
    .await?;

    Ok(Json(IngestResponse {
        session_key,
        cached_endpoints,
        generated_snapshots: built.generated_snapshots,
        status: IngestStatus::Ready,
        endpoint_coverage: built.endpoint_coverage,
        track_geometry: Some(built.track_geometry),
        available_channels: Some(built.available_channels),
        warnings: built.warnings,
        error: None,
    })
    .into_response())
}

fn failed_ingest_response(
    status_code: StatusCode,
    session_key: i64,
    cached_endpoints: usize,
    error: String,
) -> Response {
    (
        status_code,
        Json(IngestResponse {
            session_key,
            status: IngestStatus::Failed,
            cached_endpoints,
            endpoint_coverage: vec![],
            generated_snapshots: 0,
            track_geometry: None,
            available_channels: None,
            warnings: vec![],
            error: Some(error),
        }),
    )
        .into_response()
}
