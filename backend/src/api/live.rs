use super::{ApiError, AppState};
use crate::domain::ReplaySnapshot;
use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::{self, Stream, StreamExt};
use std::{collections::HashSet, convert::Infallible, time::Duration};

pub async fn current(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::LiveCurrentResponse>, ApiError> {
    Ok(Json(
        state
            .live
            .current(&state.pool)
            .await
            .map_err(ApiError::Storage)?,
    ))
}

pub async fn start(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::LiveSessionStatus>, ApiError> {
    Ok(Json(
        state
            .live
            .start(&state.pool, session_key)
            .await
            .map_err(live_start_error)?,
    ))
}

pub async fn status(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Json<crate::domain::LiveSessionStatus> {
    Json(state.live.status(session_key).await)
}

pub async fn metadata(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::ReplayMetadata>, ApiError> {
    state
        .live
        .metadata(session_key)
        .await
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn snapshot(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<ReplaySnapshot>, ApiError> {
    state
        .live
        .snapshot(&state.pool, session_key)
        .await
        .map_err(ApiError::Storage)?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn stop(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Json<crate::domain::LiveSessionStatus> {
    Json(state.live.stop(session_key).await)
}

pub async fn track_geometry(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::TrackGeometry>, ApiError> {
    state
        .live
        .geometry(session_key)
        .await
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn events(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::ReplayEventListResponse>, ApiError> {
    state
        .live
        .events(session_key)
        .await
        .map(|events| {
            Json(crate::domain::ReplayEventListResponse {
                contract_version: crate::domain::REPLAY_CONTRACT_VERSION.to_string(),
                events,
            })
        })
        .ok_or(ApiError::NotFound)
}

pub async fn stream(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let status = state.live.status(session_key).await;
    if !status.active {
        return Err(ApiError::NotFound);
    }
    let metadata = state
        .live
        .metadata(session_key)
        .await
        .ok_or(ApiError::NotFound)?;
    let metadata_event = Event::default()
        .event("metadata")
        .json_data(&metadata)
        .unwrap_or_else(|_| Event::default().event("error").data("serialization failed"));
    let registry = state.live.clone();
    let pool = state.pool.clone();
    let frame_delay = Duration::from_secs_f64(metadata.frame_step_seconds.max(0.25));
    let initial_t = status.current_t.unwrap_or(0.0);
    // Re-send the small event history on reconnect; clients dedupe by event id.
    let initial_event_ids = HashSet::new();

    let snapshots = stream::unfold(
        (initial_t, false, initial_event_ids),
        move |(previous_t, ended, mut emitted_event_ids)| {
            let registry = registry.clone();
            let pool = pool.clone();
            async move {
                if ended {
                    return None;
                }
                tokio::time::sleep(frame_delay).await;
                let snapshot = match registry.snapshot(&pool, session_key).await {
                    Ok(Some(snapshot)) => snapshot,
                    Ok(None) => {
                        return Some((
                            stream::iter(vec![Event::default()
                                .event("end")
                                .data("live session ended")]),
                            (previous_t, true, emitted_event_ids),
                        ));
                    }
                    Err(error) => {
                        return Some((
                            stream::iter(vec![Event::default()
                                .event("error")
                                .data(format!("live snapshot refresh failed: {error}"))]),
                            (previous_t, false, emitted_event_ids),
                        ));
                    }
                };
                let t = snapshot.cursor.t;
                let mut events = vec![Event::default()
                    .event("snapshot")
                    .json_data(snapshot)
                    .unwrap_or_else(|_| {
                        Event::default().event("error").data("serialization failed")
                    })];
                events.extend(
                    registry
                        .events(session_key)
                        .await
                        .into_iter()
                        .flatten()
                        .filter(|event| event.t <= t && emitted_event_ids.insert(event.id.clone()))
                        .map(|event| {
                            Event::default()
                                .event("event")
                                .json_data(event)
                                .unwrap_or_else(|_| {
                                    Event::default().event("error").data("serialization failed")
                                })
                        }),
                );
                Some((stream::iter(events), (t, false, emitted_event_ids)))
            }
        },
    )
    .flatten();

    Ok(Sse::new(
        stream::once(async { Ok::<Event, Infallible>(metadata_event) })
            .chain(snapshots.map(Ok::<Event, Infallible>)),
    )
    .keep_alive(KeepAlive::default()))
}

fn live_start_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("requested session is not the active OpenF1 live session")
        || message.contains("no active OpenF1 live session")
        || message.contains("OpenF1 live mode is disabled")
    {
        ApiError::BadRequest(message)
    } else if message.contains("OpenF1 live initial snapshot has no") {
        ApiError::ServiceUnavailable(message)
    } else if message.contains("OpenF1 live request failed")
        || message.contains("OpenF1 live configuration error")
        || message.contains("invalid OpenF1 live auth header")
        || message.contains("OpenF1 rejected the credentials")
        || message.contains("OpenF1 token endpoint")
    {
        ApiError::BadGateway(message)
    } else {
        ApiError::Storage(error)
    }
}
