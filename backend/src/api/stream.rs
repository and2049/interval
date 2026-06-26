use super::{ApiError, AppState};
use crate::{replay, storage};
use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{
    stream::{self, Stream},
    StreamExt,
};
use std::{collections::VecDeque, convert::Infallible, time::Duration};

pub async fn replay_stream(
    State(state): State<AppState>,
    Path(session_key): Path<i64>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let metadata = replay::metadata(&state.pool, session_key)
        .await?
        .ok_or(ApiError::NotFound)?;
    let pool = state.pool.clone();
    let total_frames = metadata.total_frames;
    let replay_events = storage::get_replay_events(&state.pool, session_key).await?;
    let metadata_event = Event::default()
        .event("metadata")
        .json_data(&metadata)
        .unwrap_or_else(|_| Event::default().event("error").data("serialization failed"));

    let samples = (0..total_frames).map(move |frame| {
        let pool = pool.clone();
        let events = replay_events.clone();
        let metadata = metadata.clone();
        async move {
            let window = replay::streaming::frame_window(&metadata, frame);
            let snapshot = replay::snapshot_at(&pool, session_key, window.t)
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
