mod cache_rebuild;
mod curated_tracks;
mod generator;
pub mod track_projection;

use crate::domain::{ReplayMetadata, ReplaySnapshot};
use sqlx::SqlitePool;

pub use cache_rebuild::{rebuild_from_cache, CachedReplayBuild};
pub use curated_tracks::{curated_geometry, BAHRAIN_SESSION_KEY};
pub use generator::generate_replay;

pub async fn metadata(pool: &SqlitePool, session_key: i64) -> sqlx::Result<Option<ReplayMetadata>> {
    crate::storage::get_replay_metadata(pool, session_key).await
}

pub async fn snapshot_at(
    pool: &SqlitePool,
    session_key: i64,
    t: f64,
) -> sqlx::Result<Option<ReplaySnapshot>> {
    crate::storage::get_replay_snapshot(pool, session_key, t).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connectors::openf1_historical::RawEndpoint,
        domain::{Meeting, Session},
        storage,
    };
    use serde_json::json;

    #[tokio::test]
    async fn clamps_before_start_to_first_snapshot() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();

        let snapshot = snapshot_at(&pool, 9839, -10.0).await.unwrap().unwrap();
        assert_eq!(snapshot.cursor.t, 0.0);
    }

    #[tokio::test]
    async fn clamps_after_end_to_last_snapshot() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();

        let snapshot = snapshot_at(&pool, 9839, 9_999.0).await.unwrap().unwrap();
        assert_eq!(snapshot.cursor.t, 180.0);
    }

    #[tokio::test]
    async fn picks_nearest_prior_snapshot_between_samples() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        storage::seed_demo_session(&pool).await.unwrap();

        let snapshot = snapshot_at(&pool, 9839, 75.0).await.unwrap().unwrap();
        assert_eq!(snapshot.cursor.t, 60.0);
    }

    #[tokio::test]
    async fn rebuilds_replay_from_cached_raw_bundle() {
        let pool = storage::connect("sqlite::memory:").await.unwrap();
        storage::migrate(&pool).await.unwrap();
        let session = Session {
            session_key: 42,
            meeting_key: 7,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2024-03-02T15:00:00Z".to_string(),
            end_time: "2024-03-02T17:00:00Z".to_string(),
            total_laps: 57,
        };
        storage::upsert_meetings(
            &pool,
            &[Meeting {
                meeting_key: session.meeting_key,
                year: session.year,
                name: "Test Grand Prix".to_string(),
                country: "Test".to_string(),
                location: "Test".to_string(),
            }],
        )
        .await
        .unwrap();
        storage::upsert_sessions(&pool, std::slice::from_ref(&session))
            .await
            .unwrap();
        storage::store_raw_bundle(&pool, &cached_bundle(session.session_key))
            .await
            .unwrap();

        let build = rebuild_from_cache(&pool, session.session_key)
            .await
            .unwrap();
        assert!(build.generated_snapshots > 1);
        assert!(build.available_channels.timing);
        assert!(metadata(&pool, session.session_key)
            .await
            .unwrap()
            .is_some());
        assert!(snapshot_at(&pool, session.session_key, 10.0)
            .await
            .unwrap()
            .is_some());
    }

    fn cached_bundle(session_key: i64) -> Vec<RawEndpoint> {
        let start = "2024-03-02T15:00:00Z";
        vec![
            raw(
                session_key,
                "drivers",
                json!([{
                    "driver_number": 1,
                    "full_name": "Max Verstappen",
                    "name_acronym": "VER",
                    "team_colour": "3671C6",
                    "team_name": "Red Bull Racing"
                }]),
            ),
            raw(
                session_key,
                "laps",
                json!([{
                    "driver_number": 1,
                    "lap_number": 1,
                    "date_start": "2024-03-02T15:00:05Z",
                    "lap_duration": 91.0,
                    "duration_sector_1": 18.0,
                    "duration_sector_2": 34.0,
                    "duration_sector_3": 22.0
                }]),
            ),
            raw(
                session_key,
                "intervals",
                json!([{
                    "date": "2024-03-02T15:00:05Z",
                    "driver_number": 1,
                    "gap_to_leader": null,
                    "interval": null
                }]),
            ),
            raw(
                session_key,
                "position",
                json!([{
                    "date": "2024-03-02T15:00:05Z",
                    "driver_number": 1,
                    "position": 1
                }]),
            ),
            raw(
                session_key,
                "location",
                json!([
                    { "date": start, "driver_number": 1, "x": 0.0, "y": 0.0, "z": 0.0 },
                    { "date": "2024-03-02T15:00:05Z", "driver_number": 1, "x": 10.0, "y": 0.0, "z": 0.0 },
                    { "date": "2024-03-02T15:00:10Z", "driver_number": 1, "x": 10.0, "y": 10.0, "z": 0.0 },
                    { "date": "2024-03-02T15:00:15Z", "driver_number": 1, "x": 0.0, "y": 10.0, "z": 0.0 },
                    { "date": "2024-03-02T15:00:20Z", "driver_number": 1, "x": 0.0, "y": 0.0, "z": 0.0 }
                ]),
            ),
            raw(session_key, "pit", json!([])),
            raw(session_key, "race_control", json!([])),
            raw(
                session_key,
                "stints",
                json!([{
                    "driver_number": 1,
                    "stint_number": 1,
                    "compound": "MEDIUM",
                    "lap_start": 1,
                    "lap_end": null,
                    "tyre_age_at_start": 0
                }]),
            ),
            raw(
                session_key,
                "weather",
                json!([{
                    "date": "2024-03-02T15:00:05Z",
                    "air_temperature": 20.0,
                    "track_temperature": 28.0,
                    "humidity": 40.0,
                    "rainfall": 0.0,
                    "wind_direction": 90,
                    "wind_speed": 1.5
                }]),
            ),
            raw(
                session_key,
                "session_result",
                json!([{
                    "driver_number": 1,
                    "position": 1,
                    "dnf": false,
                    "dns": false,
                    "dsq": false
                }]),
            ),
        ]
    }

    fn raw(session_key: i64, endpoint: &str, payload: serde_json::Value) -> RawEndpoint {
        RawEndpoint {
            endpoint: endpoint.to_string(),
            session_key,
            payload,
        }
    }
}
