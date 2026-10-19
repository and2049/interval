use crate::{
    domain::{DataSource, EndpointLinks, LiveSessionStatus, ReplayMetadata, ReplaySnapshot},
    replay,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;

pub const MIN_SPEED: f64 = 0.25;
pub const MAX_SPEED: f64 = 1_000.0;

/// Optional start parameters. Absent fields keep the original behavior: start at
/// `race_start_t` (or `min_t`) at 1x. `start_t` and `start_lap` are mutually exclusive.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartOptions {
    pub start_t: Option<f64>,
    pub start_lap: Option<i32>,
    pub speed: Option<f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("cached replay is required")]
    NotCached,
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
}

impl From<sqlx::Error> for SimulationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.into())
    }
}

pub fn normalize_speed(speed: f64) -> Result<f64, SimulationError> {
    if !speed.is_finite() || speed <= 0.0 {
        return Err(SimulationError::InvalidRequest(format!(
            "speed must be a positive number, got {speed}"
        )));
    }
    Ok(speed.clamp(MIN_SPEED, MAX_SPEED))
}

#[derive(Clone, Default)]
pub struct LiveSimulationRegistry {
    sessions: Arc<Mutex<HashMap<i64, LiveSimulation>>>,
}

#[derive(Debug, Clone)]
struct LiveSimulation {
    session_key: i64,
    started_instant: Instant,
    started_at: String,
    min_t: f64,
    max_t: f64,
    speed: f64,
}

impl LiveSimulationRegistry {
    pub async fn start(
        &self,
        pool: &SqlitePool,
        session_key: i64,
        options: StartOptions,
    ) -> Result<LiveSessionStatus, SimulationError> {
        let metadata = replay::metadata(pool, session_key)
            .await?
            .ok_or(SimulationError::NotCached)?;
        let speed = normalize_speed(options.speed.unwrap_or(1.0))?;
        let start_t = match (options.start_t, options.start_lap) {
            (Some(_), Some(_)) => {
                return Err(SimulationError::InvalidRequest(
                    "start_t and start_lap are mutually exclusive".to_string(),
                ));
            }
            (Some(t), None) => clamp_start_t(&metadata, t)?,
            (None, Some(lap)) => lap_start_t(pool, session_key, &metadata, lap).await?,
            (None, None) => simulation_start_t(&metadata),
        };
        replay::snapshot_at(pool, session_key, start_t)
            .await?
            .ok_or(SimulationError::NotCached)?;

        let simulation = LiveSimulation {
            session_key,
            started_instant: Instant::now(),
            started_at: chrono::Utc::now().to_rfc3339(),
            min_t: start_t,
            max_t: metadata.max_t,
            speed,
        };
        let status = simulation.status();
        self.sessions.lock().await.insert(session_key, simulation);
        Ok(status)
    }

    pub async fn stop(&self, session_key: i64) -> LiveSessionStatus {
        let removed = self.sessions.lock().await.remove(&session_key);
        removed.map_or_else(
            || inactive_status(session_key),
            |simulation| LiveSessionStatus {
                session_key,
                active: false,
                current_t: Some(simulation.current_t()),
                max_t: Some(simulation.max_t),
                started_at: Some(simulation.started_at),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
                source: Some("live_simulation".to_string()),
                channels: vec![],
            },
        )
    }

    pub async fn status(&self, session_key: i64) -> LiveSessionStatus {
        self.sessions
            .lock()
            .await
            .get(&session_key)
            .map(LiveSimulation::status)
            .unwrap_or_else(|| inactive_status(session_key))
    }

    /// Rebases the clock so the current simulation time is continuous across the change.
    /// Returns `None` when no simulation is running for the session.
    pub async fn set_speed(&self, session_key: i64, speed: f64) -> Option<LiveSessionStatus> {
        let mut sessions = self.sessions.lock().await;
        let simulation = sessions.get_mut(&session_key)?;
        let current_t = simulation.current_t();
        simulation.min_t = current_t;
        simulation.started_instant = Instant::now();
        simulation.speed = speed.max(0.001);
        Some(simulation.status())
    }

    pub async fn current_snapshot(
        &self,
        pool: &SqlitePool,
        session_key: i64,
    ) -> anyhow::Result<Option<ReplaySnapshot>> {
        let t = match self.sessions.lock().await.get(&session_key) {
            Some(simulation) => simulation.current_t(),
            None => return Ok(None),
        };
        Ok(replay::snapshot_at(pool, session_key, t).await?)
    }

    pub async fn live_metadata(
        &self,
        pool: &SqlitePool,
        session_key: i64,
    ) -> anyhow::Result<Option<ReplayMetadata>> {
        let Some(mut metadata) = replay::metadata(pool, session_key).await? else {
            return Ok(None);
        };
        metadata.data_sources = vec![DataSource {
            name: "live_simulation".to_string(),
            mode: "simulated_live".to_string(),
        }];
        metadata.endpoints = EndpointLinks {
            snapshot_endpoint: format!("/api/sessions/{session_key}/live-simulation/snapshot"),
            stream_endpoint: format!("/api/sessions/{session_key}/live-simulation/stream"),
            events_endpoint: format!("/api/sessions/{session_key}/replay/events"),
            track_geometry_endpoint: format!("/api/sessions/{session_key}/track/geometry"),
        };
        Ok(Some(metadata))
    }
}

impl LiveSimulation {
    fn current_t(&self) -> f64 {
        (self.min_t + self.started_instant.elapsed().as_secs_f64() * self.speed)
            .clamp(self.min_t, self.max_t)
    }

    fn status(&self) -> LiveSessionStatus {
        LiveSessionStatus {
            session_key: self.session_key,
            active: self.current_t() < self.max_t,
            current_t: Some(self.current_t()),
            max_t: Some(self.max_t),
            started_at: Some(self.started_at.clone()),
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            source: Some("live_simulation".to_string()),
            channels: vec![],
        }
    }
}

fn inactive_status(session_key: i64) -> LiveSessionStatus {
    LiveSessionStatus {
        session_key,
        active: false,
        current_t: None,
        max_t: None,
        started_at: None,
        updated_at: None,
        source: None,
        channels: vec![],
    }
}

fn clamp_start_t(metadata: &ReplayMetadata, t: f64) -> Result<f64, SimulationError> {
    if !t.is_finite() {
        return Err(SimulationError::InvalidRequest(format!(
            "start_t must be a finite number, got {t}"
        )));
    }
    let last_frame = (metadata.max_t - metadata.frame_step_seconds).max(metadata.min_t);
    Ok(t.clamp(metadata.min_t, last_frame))
}

/// Earliest persisted frame whose leader lap has reached `lap`, found by bisecting the
/// frame index so a long replay costs a handful of snapshot reads instead of a full scan.
async fn lap_start_t(
    pool: &SqlitePool,
    session_key: i64,
    metadata: &ReplayMetadata,
    lap: i32,
) -> Result<f64, SimulationError> {
    if lap < 1 {
        return Err(SimulationError::InvalidRequest(format!(
            "start_lap must be at least 1, got {lap}"
        )));
    }
    let step = metadata.frame_step_seconds.max(0.001);
    let frame_t = |frame: i64| metadata.min_t + frame as f64 * step;
    let lap_at = |frame: i64| async move {
        replay::snapshot_at(pool, session_key, frame_t(frame))
            .await
            .map(|snapshot| snapshot.map_or(0, |snapshot| snapshot.race_state.lap))
    };
    let last = ((metadata.max_t - metadata.min_t) / step).floor().max(0.0) as i64;
    if lap_at(last).await? < lap {
        return Err(SimulationError::InvalidRequest(format!(
            "start_lap {lap} is beyond the cached replay"
        )));
    }
    let (mut lo, mut hi) = (0, last);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if lap_at(mid).await? >= lap {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(frame_t(lo))
}

fn simulation_start_t(metadata: &ReplayMetadata) -> f64 {
    if metadata.race_start_t.is_finite()
        && metadata.race_start_t >= metadata.min_t
        && metadata.race_start_t < metadata.max_t
    {
        metadata.race_start_t
    } else {
        metadata.min_t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;

    #[tokio::test]
    async fn starts_from_cached_replay_and_returns_snapshot() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();
        let registry = LiveSimulationRegistry::default();

        let status = registry
            .start(&pool, 9839, StartOptions::default())
            .await
            .unwrap();
        let snapshot = registry
            .current_snapshot(&pool, 9839)
            .await
            .unwrap()
            .unwrap();

        assert!(status.active);
        assert_eq!(status.source.as_deref(), Some("live_simulation"));
        assert_eq!(snapshot.cursor.session_key, 9839);
    }

    #[tokio::test]
    async fn starts_at_race_start_when_metadata_has_one() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();
        let mut metadata = replay::metadata(&pool, 9839).await.unwrap().unwrap();
        metadata.race_start_t = 60.0;
        let snapshots = storage::list_replay_snapshots_from(&pool, 9839, 0.0)
            .await
            .unwrap();
        let events = storage::get_replay_events(&pool, 9839).await.unwrap();
        storage::replace_replay(&pool, &metadata, &snapshots, &events)
            .await
            .unwrap();
        let registry = LiveSimulationRegistry::default();

        let status = registry
            .start(&pool, 9839, StartOptions::default())
            .await
            .unwrap();
        let snapshot = registry
            .current_snapshot(&pool, 9839)
            .await
            .unwrap()
            .unwrap();

        assert!(status.current_t.unwrap() >= 60.0);
        assert_eq!(snapshot.cursor.t, 60.0);
    }

    async fn demo_pool() -> SqlitePool {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn start_t_is_clamped_to_the_last_persisted_frame() {
        let pool = demo_pool().await;
        let registry = LiveSimulationRegistry::default();

        let options = StartOptions {
            start_t: Some(60.0),
            ..StartOptions::default()
        };
        let status = registry.start(&pool, 9839, options).await.unwrap();
        assert!(status.current_t.unwrap() >= 60.0);

        let options = StartOptions {
            start_t: Some(10_000.0),
            ..StartOptions::default()
        };
        let status = registry.start(&pool, 9839, options).await.unwrap();
        assert!(status.active);
        assert!(status.current_t.unwrap() >= 120.0);

        let options = StartOptions {
            start_t: Some(-5.0),
            ..StartOptions::default()
        };
        let status = registry.start(&pool, 9839, options).await.unwrap();
        assert!(status.current_t.unwrap() >= 0.0);
        assert!(status.current_t.unwrap() < 1.0);
    }

    #[tokio::test]
    async fn start_lap_resolves_the_first_frame_of_that_lap() {
        let pool = demo_pool().await;
        let registry = LiveSimulationRegistry::default();

        let options = StartOptions {
            start_lap: Some(3),
            ..StartOptions::default()
        };
        registry.start(&pool, 9839, options).await.unwrap();
        let snapshot = registry
            .current_snapshot(&pool, 9839)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.race_state.lap, 3);
        assert_eq!(snapshot.cursor.t, 120.0);

        let options = StartOptions {
            start_lap: Some(99),
            ..StartOptions::default()
        };
        assert!(matches!(
            registry.start(&pool, 9839, options).await,
            Err(SimulationError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn start_rejects_conflicting_selectors_and_invalid_speed() {
        let pool = demo_pool().await;
        let registry = LiveSimulationRegistry::default();

        let options = StartOptions {
            start_t: Some(0.0),
            start_lap: Some(1),
            speed: None,
        };
        assert!(matches!(
            registry.start(&pool, 9839, options).await,
            Err(SimulationError::InvalidRequest(_))
        ));

        let options = StartOptions {
            speed: Some(0.0),
            ..StartOptions::default()
        };
        assert!(matches!(
            registry.start(&pool, 9839, options).await,
            Err(SimulationError::InvalidRequest(_))
        ));
        assert!(!registry.status(9839).await.active);
    }

    #[tokio::test]
    async fn set_speed_keeps_simulation_time_continuous() {
        let pool = demo_pool().await;
        let registry = LiveSimulationRegistry::default();
        let options = StartOptions {
            start_t: Some(60.0),
            speed: Some(4.0),
            ..StartOptions::default()
        };
        registry.start(&pool, 9839, options).await.unwrap();

        let before = registry.status(9839).await.current_t.unwrap();
        let status = registry.set_speed(9839, 1.0).await.unwrap();
        let after = status.current_t.unwrap();

        assert!(after >= before);
        assert!(after - before < 1.0);
        assert!(registry.set_speed(42, 1.0).await.is_none());
    }

    #[tokio::test]
    async fn inactive_status_is_empty() {
        let registry = LiveSimulationRegistry::default();

        let status = registry.status(123).await;

        assert!(!status.active);
        assert_eq!(status.current_t, None);
    }
}
