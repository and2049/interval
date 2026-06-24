use crate::{connectors::openf1_historical::HistoricalClient, normalization, replay, storage};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::{
    stream::{self, Stream},
    StreamExt,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{collections::VecDeque, convert::Infallible, time::Duration};
use thiserror::Error;

#[derive(Clone)]
pub struct AppState {
    pool: SqlitePool,
    historical: HistoricalClient,
}

impl AppState {
    pub fn new(pool: SqlitePool, historical: HistoricalClient) -> Self {
        Self { pool, historical }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/seasons", get(seasons))
        .route("/api/meetings", get(meetings))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{session_key}/ingest", post(ingest_session))
        .route(
            "/api/sessions/{session_key}/replay/metadata",
            get(replay_metadata),
        )
        .route(
            "/api/sessions/{session_key}/replay/snapshot",
            get(replay_snapshot),
        )
        .route(
            "/api/sessions/{session_key}/replay/events",
            get(replay_events),
        )
        .route(
            "/api/sessions/{session_key}/replay/stream",
            get(replay_stream),
        )
        .route(
            "/api/sessions/{session_key}/track/geometry",
            get(track_geometry),
        )
        .with_state(state)
}

async fn healthz() -> Json<Health> {
    Json(Health { ok: true })
}

async fn seasons(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::domain::Season>>, ApiError> {
    let mut years = vec![2026, 2025, 2024, 2023]
        .into_iter()
        .map(|year| crate::domain::Season { year })
        .collect::<Vec<_>>();
    for season in storage::list_seasons(&state.pool).await? {
        if !years.iter().any(|candidate| candidate.year == season.year) {
            years.push(season);
        }
    }
    years.sort_by(|a, b| b.year.cmp(&a.year));
    Ok(Json(years))
}

async fn meetings(
    State(state): State<AppState>,
    Query(query): Query<MeetingsQuery>,
) -> Result<Json<Vec<crate::domain::Meeting>>, ApiError> {
    let cached = storage::list_meetings(&state.pool, query.season).await?;
    if !cached.is_empty() {
        return Ok(Json(cached));
    }

    let payload = state.historical.fetch_meetings(query.season).await?;
    let meetings = normalization::meetings_from_openf1(payload)?;
    storage::upsert_meetings(&state.pool, &meetings).await?;
    Ok(Json(meetings))
}

async fn sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<Vec<crate::domain::SessionReadiness>>, ApiError> {
    let cached = storage::list_sessions(&state.pool, query.meeting_key).await?;
    if !cached.is_empty() {
        return Ok(Json(
            storage::list_session_readiness(&state.pool, query.meeting_key).await?,
        ));
    }

    let payload = state
        .historical
        .fetch_sessions_for_meeting(query.meeting_key)
        .await?;
    let sessions = normalization::race_sessions_from_openf1(payload)?;
    storage::upsert_sessions(&state.pool, &sessions).await?;
    Ok(Json(
        storage::list_session_readiness(&state.pool, query.meeting_key).await?,
    ))
}

async fn ingest_session(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::IngestResponse>, ApiError> {
    let session = storage::get_session(&state.pool, session_key)
        .await?
        .ok_or(ApiError::NotFound)?;
    let _ = session;
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
                crate::domain::IngestStatus::Failed,
                Some(&error.to_string()),
            )
            .await?;
            return Err(ApiError::Historical(error));
        }
        Err(_) => {
            let message = "historical ingest timed out while fetching OpenF1 data";
            storage::set_ingest_status(
                &state.pool,
                session_key,
                crate::domain::IngestStatus::Failed,
                Some(message),
            )
            .await?;
            return Err(ApiError::Storage(anyhow::anyhow!(message)));
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
                crate::domain::IngestStatus::Failed,
                Some(&error.to_string()),
            )
            .await?;
            return Err(ApiError::Storage(error));
        }
    };
    storage::set_ingest_status(
        &state.pool,
        session_key,
        crate::domain::IngestStatus::Ready,
        None,
    )
    .await?;

    Ok(Json(crate::domain::IngestResponse {
        session_key,
        cached_endpoints,
        generated_snapshots: built.generated_snapshots,
        status: crate::domain::IngestStatus::Ready,
        endpoint_coverage: built.endpoint_coverage,
        track_geometry: Some(built.track_geometry),
        available_channels: Some(built.available_channels),
        warnings: built.warnings,
        error: None,
    }))
}

async fn track_geometry(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::TrackGeometry>, ApiError> {
    storage::get_track_geometry(&state.pool, session_key)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn replay_metadata(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::ReplayMetadata>, ApiError> {
    replay::metadata(&state.pool, session_key)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn replay_snapshot(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
    Query(query): Query<SnapshotQuery>,
) -> Result<Json<crate::domain::ReplaySnapshot>, ApiError> {
    replay::snapshot_at(&state.pool, session_key, query.t)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn replay_events(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::ReplayEventListResponse>, ApiError> {
    Ok(Json(crate::domain::ReplayEventListResponse {
        contract_version: crate::domain::REPLAY_CONTRACT_VERSION.to_string(),
        events: storage::get_replay_events(&state.pool, session_key).await?,
    }))
}

async fn replay_stream(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let metadata = replay::metadata(&state.pool, session_key)
        .await?
        .ok_or(ApiError::NotFound)?;
    let pool = state.pool.clone();
    let min_t = metadata.min_t;
    let frame_step = metadata.frame_step_seconds.max(1.0);
    let total_frames = metadata.total_frames;
    let replay_events = storage::get_replay_events(&state.pool, session_key).await?;
    let metadata_event = Event::default()
        .event("metadata")
        .json_data(&metadata)
        .unwrap_or_else(|_| Event::default().event("error").data("serialization failed"));

    let samples = (0..total_frames).map(move |frame| {
        let pool = pool.clone();
        let events = replay_events.clone();
        async move {
            let t = min_t + frame as f64 * frame_step;
            let previous_t = if frame == 0 {
                f64::NEG_INFINITY
            } else {
                min_t + (frame - 1) as f64 * frame_step
            };
            let snapshot = replay::snapshot_at(&pool, session_key, t)
                .await
                .ok()
                .flatten();
            let mut out = VecDeque::new();
            out.push_back(match snapshot {
                Some(snapshot) => Event::default()
                    .event("snapshot")
                    .json_data(snapshot)
                    .unwrap_or_else(|_| {
                        Event::default().event("error").data("serialization failed")
                    }),
                None => Event::default().event("error").data("no snapshot"),
            });
            for replay_event in events
                .iter()
                .filter(|event| event.t > previous_t && event.t <= t)
            {
                out.push_back(
                    Event::default()
                        .event("event")
                        .json_data(replay_event)
                        .unwrap_or_else(|_| {
                            Event::default().event("error").data("serialization failed")
                        }),
                );
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            stream::iter(out.into_iter().map(Ok::<Event, Infallible>))
        }
    });
    let end = stream::once(async { Ok::<Event, Infallible>(Event::default().event("end")) });

    Ok(Sse::new(
        stream::once(async { Ok::<Event, Infallible>(metadata_event) })
            .chain(stream::iter(samples).then(|future| future).flatten())
            .chain(end),
    )
    .keep_alive(KeepAlive::default()))
}

#[derive(Debug, Deserialize)]
struct MeetingsQuery {
    season: i32,
}

#[derive(Debug, Deserialize)]
struct SessionsQuery {
    meeting_key: i64,
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    t: f64,
}

#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("resource not found")]
    NotFound,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("storage error: {0}")]
    Storage(#[from] anyhow::Error),
    #[error("historical connector error: {0}")]
    Historical(#[from] crate::connectors::openf1_historical::HistoricalError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Historical(_) => StatusCode::BAD_GATEWAY,
            ApiError::Database(_) | ApiError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = Json(serde_json::json!({
            "error": self.to_string()
        }));
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;

    #[tokio::test]
    async fn metadata_endpoint_has_seeded_demo_session() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();

        let metadata = replay::metadata(&pool, 9839).await.unwrap().unwrap();
        assert_eq!(
            metadata.session.session_type,
            crate::domain::SessionType::Race
        );
        assert_eq!(
            metadata.contract_version,
            crate::domain::REPLAY_CONTRACT_VERSION
        );
        assert_eq!(metadata.drivers.len(), 5);
        assert!(metadata.available_channels.timing);
        assert_eq!(
            metadata.track_geometry.status,
            crate::domain::TrackGeometryQuality::Schematic
        );
        assert!(metadata
            .endpoints
            .snapshot_endpoint
            .contains("/replay/snapshot"));
    }

    #[tokio::test]
    async fn events_are_versioned_envelopes() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();

        let events = storage::get_replay_events(&pool, 9839).await.unwrap();
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, crate::domain::EventKind::RaceControl);
        assert_eq!(events[0].source, crate::domain::EventSource::System);
    }

    #[tokio::test]
    async fn session_list_includes_readiness() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();
        storage::seed_mvp_fixture(&pool).await.unwrap();

        let demo = storage::list_session_readiness(&pool, 1276)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(demo.is_demo);
        assert!(demo.replay_ready);
        assert_eq!(demo.ingest_status, crate::domain::IngestStatus::Ready);

        let fixture = storage::list_session_readiness(&pool, 1229)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(!fixture.is_demo);
        assert!(!fixture.replay_ready);
        assert_eq!(
            fixture.ingest_status,
            crate::domain::IngestStatus::NotIngested
        );
    }
}
