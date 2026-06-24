use crate::{
    domain::{EndpointCoverage, IngestTrackGeometrySummary},
    normalization, replay, storage,
};
use sqlx::SqlitePool;

pub struct CachedReplayBuild {
    pub generated_snapshots: usize,
    pub endpoint_coverage: Vec<EndpointCoverage>,
    pub track_geometry: IngestTrackGeometrySummary,
    pub available_channels: crate::domain::AvailableChannels,
    pub warnings: Vec<String>,
}

pub async fn rebuild_from_cache(
    pool: &SqlitePool,
    session_key: i64,
) -> anyhow::Result<CachedReplayBuild> {
    let session = storage::get_session(pool, session_key)
        .await?
        .ok_or_else(|| anyhow::anyhow!("session {session_key} not found"))?;
    let bundle = storage::load_raw_bundle(pool, session_key).await?;
    if bundle.is_empty() {
        anyhow::bail!("no cached raw OpenF1 data for session {session_key}");
    }

    let coverage = endpoint_coverage(&bundle);
    let race_data = normalization::race_data_from_bundle(&bundle, &session)?;
    let generated = replay::generate_replay(session, race_data)?;
    let warnings = coverage_warnings(&coverage, &generated.metadata.available_channels);
    let track_geometry = IngestTrackGeometrySummary {
        status: generated.track_geometry.quality.clone(),
        source: generated.track_geometry.source.clone(),
        quality: generated.track_geometry.quality.clone(),
    };
    let available_channels = generated.metadata.available_channels.clone();
    let generated_snapshots = generated.snapshots.len();

    storage::upsert_sessions(pool, std::slice::from_ref(&generated.session)).await?;
    storage::replace_replay(
        pool,
        &generated.metadata,
        &generated.snapshots,
        &generated.events,
    )
    .await?;
    storage::replace_track_geometry(pool, &generated.track_geometry).await?;

    Ok(CachedReplayBuild {
        generated_snapshots,
        endpoint_coverage: coverage,
        track_geometry,
        available_channels,
        warnings,
    })
}

fn endpoint_coverage(
    bundle: &[crate::connectors::openf1_historical::RawEndpoint],
) -> Vec<EndpointCoverage> {
    bundle
        .iter()
        .map(|entry| EndpointCoverage {
            endpoint: entry.endpoint.clone(),
            present: rows_in_payload(&entry.payload).is_some_and(|rows| rows > 0),
            rows: rows_in_payload(&entry.payload),
        })
        .collect()
}

fn rows_in_payload(payload: &serde_json::Value) -> Option<usize> {
    payload.as_array().map(Vec::len)
}

fn coverage_warnings(
    coverage: &[EndpointCoverage],
    channels: &crate::domain::AvailableChannels,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !channels.location {
        warnings
            .push("location channel missing; track map will use schematic fallback".to_string());
    }
    if !channels.weather {
        warnings.push("weather channel missing".to_string());
    }
    if !channels.race_control {
        warnings.push("race-control channel missing".to_string());
    }
    if !channels.intervals {
        warnings.push("interval channel missing; timing gaps may be incomplete".to_string());
    }
    for endpoint in coverage.iter().filter(|endpoint| !endpoint.present) {
        warnings.push(format!(
            "OpenF1 endpoint '{}' returned no rows",
            endpoint.endpoint
        ));
    }
    warnings.sort();
    warnings.dedup();
    warnings
}
