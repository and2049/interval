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

    let coverage = super::ingest_summary::endpoint_coverage(&bundle);
    let race_data = normalization::race_data_from_bundle(&bundle, &session)?;
    let mut generated = replay::generate_replay(session, race_data)?;
    generated.metadata.meeting = storage::get_meeting(pool, generated.session.meeting_key).await?;
    let warnings =
        super::ingest_summary::coverage_warnings(&coverage, &generated.metadata.available_channels);
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
