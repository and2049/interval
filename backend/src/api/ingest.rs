use super::{ApiError, AppState};
use crate::{
    domain::{IngestResponse, IngestStatus, SessionSupportStatus},
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
    let session = storage::get_session(&state.pool, session_key)
        .await?
        .ok_or(ApiError::NotFound)?;
    let meeting = storage::get_meeting(&state.pool, session.meeting_key).await?;
    let support = crate::domain::session_support(&session, meeting.as_ref(), chrono::Utc::now());
    if support.status != SessionSupportStatus::Supported {
        let message = support.reason.unwrap_or_else(|| {
            "This session is not available for historical replay ingest.".to_string()
        });
        storage::set_ingest_status(
            &state.pool,
            session_key,
            IngestStatus::Failed,
            Some(&message),
        )
        .await?;
        return Ok(failed_ingest_response(
            StatusCode::CONFLICT,
            session_key,
            0,
            message,
        ));
    }
    storage::set_ingest_status(
        &state.pool,
        session_key,
        crate::domain::IngestStatus::Fetching,
        None,
    )
    .await?;

    let bundle = match tokio::time::timeout(
        Duration::from_secs(1_000),
        state.fastf1.fetch_race_bundle(&session, meeting.as_ref()),
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
            let message = "historical ingest timed out while fetching FastF1 data";
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
    let resolver_warnings = fastf1_metadata_warnings(&bundle);
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

    let mut warnings = built.warnings;
    warnings.extend(resolver_warnings);

    Ok(Json(IngestResponse {
        session_key,
        cached_endpoints,
        generated_snapshots: built.generated_snapshots,
        status: IngestStatus::Ready,
        endpoint_coverage: built.endpoint_coverage,
        track_geometry: Some(built.track_geometry),
        available_channels: Some(built.available_channels),
        warnings,
        error: None,
    })
    .into_response())
}

fn fastf1_metadata_warnings(
    bundle: &[crate::connectors::openf1_historical::RawEndpoint],
) -> Vec<String> {
    let Some(metadata) = bundle
        .iter()
        .find(|entry| entry.endpoint == "fastf1_metadata")
        .map(|entry| &entry.payload)
    else {
        return Vec::new();
    };

    let mut messages = metadata
        .get("warnings")
        .and_then(|warnings| warnings.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if let Some(summary) = fastf1_resolver_summary(metadata) {
        messages.push(summary);
    }

    messages.sort();
    messages.dedup();
    messages
}

fn fastf1_resolver_summary(metadata: &serde_json::Value) -> Option<String> {
    let resolver = metadata.get("resolver")?;
    let method = resolver.get("match_method")?.as_str()?;
    let round = resolver.get("round")?.as_i64()?;
    let event = resolver
        .get("event_name")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("selected event");
    let confidence = resolver
        .get("match_confidence")
        .and_then(|value| value.as_f64())
        .map(|value| format!(" confidence {:.3}", value))
        .unwrap_or_default();

    Some(format!(
        "FastF1 resolved {event} to round {round} via {method}.{confidence}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::openf1_historical::RawEndpoint;
    use serde_json::json;

    #[test]
    fn fastf1_metadata_warnings_include_schedule_resolution() {
        let warnings = fastf1_metadata_warnings(&[RawEndpoint {
            endpoint: "fastf1_metadata".to_string(),
            session_key: 9999,
            payload: json!({
                "warnings": ["approximate match"],
                "resolver": {
                    "round": 4,
                    "event_name": "Japanese Grand Prix",
                    "match_method": "fastf1_schedule_match",
                    "match_confidence": 0.917
                }
            }),
        }]);

        assert!(warnings
            .iter()
            .any(|message| message == "approximate match"));
        assert!(warnings.iter().any(|message| {
            message.contains("Japanese Grand Prix")
                && message.contains("round 4")
                && message.contains("fastf1_schedule_match")
        }));
    }

    #[test]
    fn fastf1_metadata_warnings_dedupe_connector_messages() {
        let warnings = fastf1_metadata_warnings(&[RawEndpoint {
            endpoint: "fastf1_metadata".to_string(),
            session_key: 9472,
            payload: json!({
                "warnings": ["same", "same"],
                "resolver": {
                    "round": 1,
                    "event_name": "selected event",
                    "match_method": "curated_override",
                    "match_confidence": 1.0
                }
            }),
        }]);

        assert_eq!(
            warnings.iter().filter(|message| *message == "same").count(),
            1
        );
    }
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
