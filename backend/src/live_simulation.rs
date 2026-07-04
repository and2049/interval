use crate::{
    domain::{DataSource, EndpointLinks, LiveSessionStatus, ReplayMetadata, ReplaySnapshot},
    replay,
};
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;

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
    ) -> anyhow::Result<LiveSessionStatus> {
        let metadata = replay::metadata(pool, session_key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("cached replay metadata is required"))?;
        let start_t = simulation_start_t(&metadata);
        replay::snapshot_at(pool, session_key, start_t)
            .await?
            .ok_or_else(|| anyhow::anyhow!("cached replay snapshots are required"))?;

        let simulation = LiveSimulation {
            session_key,
            started_instant: Instant::now(),
            started_at: chrono::Utc::now().to_rfc3339(),
            min_t: start_t,
            max_t: metadata.max_t,
            speed: 1.0,
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

    pub async fn set_speed(&self, session_key: i64, speed: f64) {
        if let Some(simulation) = self.sessions.lock().await.get_mut(&session_key) {
            let current_t = simulation.current_t();
            simulation.min_t = current_t;
            simulation.started_instant = Instant::now();
            simulation.speed = speed.max(0.001);
        }
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

        let status = registry.start(&pool, 9839).await.unwrap();
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

        let status = registry.start(&pool, 9839).await.unwrap();
        let snapshot = registry
            .current_snapshot(&pool, 9839)
            .await
            .unwrap()
            .unwrap();

        assert!(status.current_t.unwrap() >= 60.0);
        assert_eq!(snapshot.cursor.t, 60.0);
    }

    #[tokio::test]
    async fn inactive_status_is_empty() {
        let registry = LiveSimulationRegistry::default();

        let status = registry.status(123).await;

        assert!(!status.active);
        assert_eq!(status.current_t, None);
    }
}
