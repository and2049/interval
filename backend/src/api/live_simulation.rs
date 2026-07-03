use super::{ApiError, AppState};
use crate::{domain::ReplaySnapshot, replay, storage};
use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Deserialize;
use std::{collections::VecDeque, convert::Infallible, time::Duration};

#[derive(Debug, Default, Deserialize)]
pub struct LiveSimulationStreamQuery {
    speed: Option<f64>,
}

pub async fn start(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<crate::domain::LiveSessionStatus>, ApiError> {
    if replay::metadata(&state.pool, session_key).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    Ok(Json(
        state
            .live_simulation
            .start(&state.pool, session_key)
            .await
            .map_err(ApiError::Storage)?,
    ))
}

pub async fn status(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Json<crate::domain::LiveSessionStatus> {
    Json(state.live_simulation.status(session_key).await)
}

pub async fn snapshot(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Json<ReplaySnapshot>, ApiError> {
    state
        .live_simulation
        .current_snapshot(&state.pool, session_key)
        .await
        .map_err(ApiError::Storage)?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn stop(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Json<crate::domain::LiveSessionStatus> {
    Json(state.live_simulation.stop(session_key).await)
}

pub async fn stream(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
    Query(query): Query<LiveSimulationStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let metadata = state
        .live_simulation
        .live_metadata(&state.pool, session_key)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;
    let status = state.live_simulation.status(session_key).await;
    if !status.active {
        return Err(ApiError::NotFound);
    }

    let frame_step = metadata.frame_step_seconds.max(0.001);
    let speed = query
        .speed
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0)
        .clamp(0.25, 1_000.0);
    state.live_simulation.set_speed(session_key, speed).await;
    let frame_delay = Duration::from_secs_f64((frame_step / speed).clamp(0.01, 2.0));
    let replay_events = storage::get_replay_events(&state.pool, session_key).await?;
    let metadata_event = Event::default()
        .event("metadata")
        .json_data(&metadata)
        .unwrap_or_else(|_| Event::default().event("error").data("serialization failed"));

    let registry = state.live_simulation.clone();
    let pool = state.pool.clone();
    let metadata_for_stream = metadata.clone();
    let events_for_stream = replay_events.clone();

    let snapshots = stream::unfold(false, move |mut emitted_final| {
        let registry = registry.clone();
        let pool = pool.clone();
        let metadata = metadata_for_stream.clone();
        let events = events_for_stream.clone();
        async move {
            if emitted_final {
                return None;
            }
            tokio::time::sleep(frame_delay).await;
            let snapshot = registry
                .current_snapshot(&pool, session_key)
                .await
                .ok()
                .flatten()?;
            let status = registry.status(session_key).await;
            emitted_final = !status.active;

            let frame = snapshot.cursor.frame_index;
            let window = replay::streaming::frame_window(&metadata, frame);
            let mut out = VecDeque::new();
            out.push_back(
                Event::default()
                    .event("snapshot")
                    .json_data(snapshot)
                    .unwrap_or_else(|_| {
                        Event::default().event("error").data("serialization failed")
                    }),
            );
            for replay_event in replay::streaming::events_for_window(&events, window) {
                out.push_back(
                    Event::default()
                        .event("event")
                        .json_data(replay_event)
                        .unwrap_or_else(|_| {
                            Event::default().event("error").data("serialization failed")
                        }),
                );
            }
            Some((
                stream::iter(out.into_iter().map(Ok::<Event, Infallible>)),
                emitted_final,
            ))
        }
    });

    let end = stream::once(async { Ok::<Event, Infallible>(Event::default().event("end")) });
    Ok(Sse::new(
        stream::once(async { Ok::<Event, Infallible>(metadata_event) })
            .chain(snapshots.flatten())
            .chain(end),
    )
    .keep_alive(KeepAlive::default()))
}
