use super::{ApiError, AppState};
use crate::{domain::ReplaySnapshot, replay, storage};
use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::stream::{self, Stream, StreamExt};
use std::{collections::VecDeque, convert::Infallible, time::Duration};

const SNAPSHOT_PAGE_SIZE: i64 = 200;

#[derive(Debug, Default, serde::Deserialize)]
pub struct ReplayStreamQuery {
    from: Option<f64>,
    speed: Option<f64>,
}

struct PageState {
    pool: sqlx::SqlitePool,
    session_key: i64,
    next_from_t: f64,
    frame_step: f64,
    buffer: VecDeque<ReplaySnapshot>,
    exhausted: bool,
}

pub async fn replay_stream(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
    Query(query): Query<ReplayStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let metadata = replay::metadata(&state.pool, session_key)
        .await?
        .ok_or(ApiError::NotFound)?;
    let total_frames = metadata.total_frames;
    let frame_step = metadata.frame_step_seconds.max(0.001);
    let from = query
        .from
        .filter(|value| value.is_finite())
        .unwrap_or(metadata.min_t)
        .clamp(metadata.min_t, metadata.max_t);
    let speed = query
        .speed
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0)
        .clamp(0.25, 16.0);
    let start_frame = (((from - metadata.min_t) / frame_step).floor() as i64)
        .clamp(0, total_frames.saturating_sub(1));
    let start_t = metadata.min_t + start_frame as f64 * frame_step;
    let frame_delay = Duration::from_secs_f64((frame_step / speed).clamp(0.05, 2.0));
    let replay_events = storage::get_replay_events(&state.pool, session_key).await?;
    let metadata_event = Event::default()
        .event("metadata")
        .json_data(&metadata)
        .unwrap_or_else(|_| Event::default().event("error").data("serialization failed"));

    let snapshot_stream = stream::unfold(
        PageState {
            pool: state.pool.clone(),
            session_key,
            next_from_t: start_t,
            frame_step,
            buffer: VecDeque::new(),
            exhausted: false,
        },
        move |mut state| async move {
            if let Some(snapshot) = state.buffer.pop_front() {
                return Some((snapshot, state));
            }
            if state.exhausted {
                return None;
            }
            let page = storage::list_replay_snapshots_page(
                &state.pool,
                state.session_key,
                state.next_from_t,
                SNAPSHOT_PAGE_SIZE,
            )
            .await
            .ok()?;

            if page.is_empty() {
                return None;
            }
            if (page.len() as i64) < SNAPSHOT_PAGE_SIZE {
                state.exhausted = true;
            }
            if let Some(last) = page.last() {
                state.next_from_t = last.cursor.t + state.frame_step;
            }
            let mut buffer = VecDeque::from(page);
            let first = buffer.pop_front()?;
            state.buffer = buffer;
            Some((first, state))
        },
    );

    let metadata_for_stream = metadata.clone();
    let events_for_stream = replay_events.clone();

    let samples = snapshot_stream.map(move |snapshot| {
        let events = events_for_stream.clone();
        let metadata = metadata_for_stream.clone();
        async move {
            let frame = snapshot.cursor.frame_index;
            if frame > start_frame {
                tokio::time::sleep(frame_delay).await;
            }
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
            stream::iter(out.into_iter().map(Ok::<Event, Infallible>))
        }
    });

    let end = stream::once(async { Ok::<Event, Infallible>(Event::default().event("end")) });

    Ok(Sse::new(
        stream::once(async { Ok::<Event, Infallible>(metadata_event) })
            .chain(samples.then(|future| future).flatten())
            .chain(end),
    )
    .keep_alive(KeepAlive::default()))
}
