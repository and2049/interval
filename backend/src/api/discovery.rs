use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::{normalization, storage};

use super::{ApiError, AppState};

pub async fn seasons(
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

pub async fn meetings(
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

pub async fn sessions(
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

#[derive(Debug, Deserialize)]
pub struct MeetingsQuery {
    season: i32,
}

#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    meeting_key: i64,
}
