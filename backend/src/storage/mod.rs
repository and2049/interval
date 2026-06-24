mod curated_fixture;
mod demo;
mod ingest_status_store;
mod raw_cache;
mod replay_store;
mod schema;
mod sessions;
mod track_geometry_store;

pub use demo::{seed_demo_session, DEMO_SESSION_KEY};
pub use ingest_status_store::{get_ingest_status, set_ingest_status};
pub use raw_cache::{load_raw_bundle, store_raw_bundle};
pub use replay_store::{
    get_replay_events, get_replay_metadata, get_replay_snapshot, replace_replay,
};
pub use schema::{connect, migrate};
pub use sessions::{
    get_session, list_meetings, list_seasons, list_session_readiness, list_sessions,
    upsert_meetings, upsert_sessions,
};
pub use track_geometry_store::{get_track_geometry, replace_track_geometry};

fn decode<T: serde::de::DeserializeOwned>(payload: String) -> sqlx::Result<T> {
    serde_json::from_str(&payload).map_err(|error| sqlx::Error::Decode(Box::new(error)))
}
pub use curated_fixture::{seed_mvp_fixture, MVP_SESSION_KEY};
