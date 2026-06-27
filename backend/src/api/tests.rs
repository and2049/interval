use super::*;
use crate::{connectors::openf1_historical::RawEndpoint, replay, storage};
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn metadata_endpoint_has_seeded_demo_session() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_demo_session(&pool).await.unwrap();

    let metadata = replay::metadata(&pool, 9839).await.unwrap().unwrap();
    assert_eq!(
        metadata.session.session_type,
        crate::domain::SessionType::Race
    );
    assert_eq!(
        metadata.contract_version,
        crate::domain::REPLAY_CONTRACT_VERSION
    );
    assert_eq!(metadata.drivers.len(), 5);
    assert!(metadata.available_channels.timing);
    assert_eq!(
        metadata.track_geometry.status,
        crate::domain::TrackGeometryQuality::Schematic
    );
    assert!(metadata
        .endpoints
        .snapshot_endpoint
        .contains("/replay/snapshot"));
}

#[tokio::test]
async fn events_are_versioned_envelopes() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_demo_session(&pool).await.unwrap();

    let events = storage::get_replay_events(&pool, 9839).await.unwrap();
    assert!(!events.is_empty());
    assert_eq!(events[0].kind, crate::domain::EventKind::RaceControl);
    assert_eq!(events[0].source, crate::domain::EventSource::System);
}

#[tokio::test]
async fn session_list_includes_readiness() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_demo_session(&pool).await.unwrap();
    storage::seed_mvp_fixture(&pool).await.unwrap();

    let demo = storage::list_session_readiness(&pool, 1276)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert!(demo.is_demo);
    assert!(demo.replay_ready);
    assert_eq!(demo.ingest_status, crate::domain::IngestStatus::Ready);

    let fixture = storage::list_session_readiness(&pool, 1229)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert!(!fixture.is_demo);
    assert!(!fixture.replay_ready);
    assert_eq!(
        fixture.ingest_status,
        crate::domain::IngestStatus::NotIngested
    );
}

#[tokio::test]
async fn session_readiness_lists_races_and_sprints() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let meeting = crate::domain::Meeting {
        meeting_key: 2400,
        year: 2024,
        name: "Miami Grand Prix".to_string(),
        country: "United States".to_string(),
        location: "Miami".to_string(),
    };
    let race = crate::domain::Session {
        session_key: 20_001,
        meeting_key: meeting.meeting_key,
        year: meeting.year,
        name: "Race".to_string(),
        session_type: crate::domain::SessionType::Race,
        start_time: "2024-05-05T20:00:00Z".to_string(),
        end_time: "2024-05-05T22:00:00Z".to_string(),
        total_laps: 57,
    };
    let sprint = crate::domain::Session {
        session_key: 20_002,
        meeting_key: meeting.meeting_key,
        year: meeting.year,
        name: "Sprint".to_string(),
        session_type: crate::domain::SessionType::Sprint,
        start_time: "2024-05-04T16:00:00Z".to_string(),
        end_time: "2024-05-04T17:00:00Z".to_string(),
        total_laps: 19,
    };
    storage::upsert_meetings(&pool, &[meeting]).await.unwrap();
    storage::upsert_sessions(&pool, &[race, sprint])
        .await
        .unwrap();

    let readiness = storage::list_session_readiness(&pool, 2400).await.unwrap();

    assert_eq!(readiness.len(), 2);
    assert_eq!(readiness[0].session.session_key, 20_002);
    assert_eq!(
        readiness[0].session.session_type,
        crate::domain::SessionType::Sprint
    );
    assert_eq!(readiness[1].session.session_key, 20_001);
    assert_eq!(
        readiness[1].session.session_type,
        crate::domain::SessionType::Race
    );
}

#[tokio::test]
async fn ingest_failure_returns_structured_response() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_mvp_fixture(&pool).await.unwrap();
    let historical = crate::connectors::openf1_historical::HistoricalClient::with_base_url(
        "http://127.0.0.1:1/v1/".parse().unwrap(),
    );
    let fastf1 = crate::connectors::fastf1_historical::FastF1HistoricalClient::for_test(
        std::env::current_dir().unwrap(),
        Some(std::path::PathBuf::from("missing-fastf1-python")),
    );
    let app = router(AppState::new_with_fastf1(pool.clone(), historical, fastf1));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/9472/ingest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert_eq!(payload["session_key"], 9472);
    assert_eq!(payload["status"], "failed");
    assert_eq!(payload["cached_endpoints"], 0);
    assert_eq!(payload["generated_snapshots"], 0);
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("FastF1 filesystem error")));

    let readiness = storage::list_session_readiness(&pool, 1229)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(readiness.ingest_status, crate::domain::IngestStatus::Failed);
    assert!(readiness
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("FastF1 filesystem error")));
}

#[tokio::test]
async fn non_bahrain_ingest_failure_records_selected_session_error() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let meeting = crate::domain::Meeting {
        meeting_key: 2300,
        year: 2024,
        name: "Italian Grand Prix".to_string(),
        country: "Italy".to_string(),
        location: "Monza".to_string(),
    };
    let session = crate::domain::Session {
        session_key: 10100,
        meeting_key: meeting.meeting_key,
        year: meeting.year,
        name: "Race".to_string(),
        session_type: crate::domain::SessionType::Race,
        start_time: "2024-09-01T13:00:00Z".to_string(),
        end_time: "2024-09-01T15:00:00Z".to_string(),
        total_laps: 53,
    };
    storage::upsert_meetings(&pool, &[meeting]).await.unwrap();
    storage::upsert_sessions(&pool, &[session]).await.unwrap();
    let historical = crate::connectors::openf1_historical::HistoricalClient::with_base_url(
        "http://127.0.0.1:1/v1/".parse().unwrap(),
    );
    let fastf1 = crate::connectors::fastf1_historical::FastF1HistoricalClient::for_test(
        std::env::current_dir().unwrap(),
        Some(std::path::PathBuf::from("missing-fastf1-python")),
    );
    let app = router(AppState::new_with_fastf1(pool.clone(), historical, fastf1));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/10100/ingest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert_eq!(payload["session_key"], 10100);
    assert_eq!(payload["status"], "failed");

    let readiness = storage::list_session_readiness(&pool, 2300)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(readiness.session.session_key, 10100);
    assert_eq!(readiness.ingest_status, crate::domain::IngestStatus::Failed);
    assert!(readiness
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("FastF1 filesystem error")));
}

#[tokio::test]
async fn sprint_ingest_failure_records_selected_session_error() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let meeting = crate::domain::Meeting {
        meeting_key: 2400,
        year: 2024,
        name: "Miami Grand Prix".to_string(),
        country: "United States".to_string(),
        location: "Miami".to_string(),
    };
    let session = crate::domain::Session {
        session_key: 20100,
        meeting_key: meeting.meeting_key,
        year: meeting.year,
        name: "Sprint".to_string(),
        session_type: crate::domain::SessionType::Sprint,
        start_time: "2024-05-04T16:00:00Z".to_string(),
        end_time: "2024-05-04T17:00:00Z".to_string(),
        total_laps: 19,
    };
    storage::upsert_meetings(&pool, &[meeting]).await.unwrap();
    storage::upsert_sessions(&pool, &[session]).await.unwrap();
    let historical = crate::connectors::openf1_historical::HistoricalClient::with_base_url(
        "http://127.0.0.1:1/v1/".parse().unwrap(),
    );
    let fastf1 = crate::connectors::fastf1_historical::FastF1HistoricalClient::for_test(
        std::env::current_dir().unwrap(),
        Some(std::path::PathBuf::from("missing-fastf1-python")),
    );
    let app = router(AppState::new_with_fastf1(pool.clone(), historical, fastf1));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/20100/ingest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert_eq!(payload["session_key"], 20100);
    assert_eq!(payload["status"], "failed");

    let readiness = storage::list_session_readiness(&pool, 2400)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(readiness.session.session_key, 20100);
    assert_eq!(
        readiness.session.session_type,
        crate::domain::SessionType::Sprint
    );
    assert_eq!(readiness.ingest_status, crate::domain::IngestStatus::Failed);
    assert!(readiness
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("FastF1 filesystem error")));
}

#[tokio::test]
async fn public_replay_routes_return_nested_v1_contracts() {
    let app = seeded_router().await;

    let metadata = get_json(
        app.clone(),
        "/api/sessions/9839/replay/metadata",
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        metadata["contract_version"],
        crate::domain::REPLAY_CONTRACT_VERSION
    );
    assert_eq!(metadata["session"]["session_key"], 9839);
    assert_eq!(metadata["track_geometry"]["status"], "schematic");
    assert!(metadata.get("track_geometry_status").is_none());
    assert!(metadata["endpoints"]["snapshot_endpoint"]
        .as_str()
        .is_some_and(|endpoint| endpoint.contains("/replay/snapshot")));

    let snapshot = get_json(
        app.clone(),
        "/api/sessions/9839/replay/snapshot?t=75",
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        snapshot["contract_version"],
        crate::domain::REPLAY_CONTRACT_VERSION
    );
    assert_eq!(snapshot["cursor"]["t"], 60.0);
    assert!(snapshot["race_state"]["lap"].is_number());
    assert!(snapshot["timing"]["rows"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
    assert!(snapshot["track"]["positions"]
        .as_array()
        .is_some_and(|positions| !positions.is_empty()));
    assert!(snapshot.get("drivers").is_none());
    assert!(snapshot.get("positions").is_none());
    assert!(snapshot.get("lap").is_none());
    assert!(snapshot.get("track_status").is_none());

    let events = get_json(
        app.clone(),
        "/api/sessions/9839/replay/events",
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        events["contract_version"],
        crate::domain::REPLAY_CONTRACT_VERSION
    );
    assert!(events["events"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let geometry = get_json(app, "/api/sessions/9839/track/geometry", StatusCode::OK).await;
    assert_eq!(
        geometry["contract_version"],
        crate::domain::REPLAY_CONTRACT_VERSION
    );
    assert_eq!(geometry["source"], "schematic");
    assert_eq!(geometry["map_mode"], "schematic");
}

#[tokio::test]
async fn public_replay_routes_return_projected_bahrain_when_location_is_missing() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_mvp_fixture(&pool).await.unwrap();
    storage::store_raw_bundle(
        &pool,
        &cached_bahrain_bundle_without_location(storage::MVP_SESSION_KEY),
    )
    .await
    .unwrap();
    let build = replay::rebuild_from_cache(&pool, storage::MVP_SESSION_KEY)
        .await
        .unwrap();
    assert!(build.available_channels.track_geometry);
    assert!(!build.available_channels.location);

    let app = router(AppState::new(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
    ));

    let metadata = get_json(
        app.clone(),
        "/api/sessions/9472/replay/metadata",
        StatusCode::OK,
    )
    .await;
    assert_eq!(metadata["track_geometry"]["source"], "curated_static");
    assert_eq!(metadata["track_geometry"]["status"], "ready");
    assert_eq!(metadata["available_channels"]["location"], false);
    assert_eq!(metadata["meeting"]["name"], "Bahrain Grand Prix");

    let snapshot = get_json(
        app.clone(),
        "/api/sessions/9472/replay/snapshot?t=10",
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["track"]["map_mode"], "projected");
    assert_eq!(snapshot["track"]["positions"][0]["source"], "projected");
    assert_eq!(snapshot["track"]["positions"][0]["quality"], "projected");
    assert!(snapshot["track"]["positions"][0]["relative_distance"].is_number());

    let geometry = get_json(app, "/api/sessions/9472/track/geometry", StatusCode::OK).await;
    assert_eq!(geometry["source"], "curated_static");
    assert_eq!(geometry["quality"], "ready");
    assert_eq!(geometry["map_mode"], "projected");
    assert!(geometry["centerline"]
        .as_array()
        .is_some_and(|points| points.len() > 20));
}

#[tokio::test]
async fn public_replay_routes_return_not_found_for_missing_replay() {
    let app = seeded_router().await;

    for uri in [
        "/api/sessions/404/replay/metadata",
        "/api/sessions/404/replay/events",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn replay_snapshot_rejects_non_finite_time() {
    let app = seeded_router().await;

    let payload = get_json(
        app,
        "/api/sessions/9839/replay/snapshot?t=NaN",
        StatusCode::BAD_REQUEST,
    )
    .await;

    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("must be a finite number")));
}

#[tokio::test]
async fn public_replay_stream_emits_v1_metadata_snapshots_events_and_end() {
    let app = seeded_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/9839/replay/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(text.contains("event: metadata"));
    assert!(text.contains("event: snapshot"));
    assert!(text.contains("event: event"));
    assert!(text.contains("event: end"));
    assert!(text.contains("\"contract_version\":\"replay.v1\""));

    let metadata = sse_payloads(&text, "metadata");
    assert_eq!(metadata.len(), 1);
    assert!(metadata[0].get("track_geometry").is_some());
    assert!(metadata[0].get("track_geometry_status").is_none());

    let snapshots = sse_payloads(&text, "snapshot");
    assert!(!snapshots.is_empty());
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.get("race_state").is_some()));
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.get("timing").is_some()));
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.get("track").is_some()));
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.get("drivers").is_none()));
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.get("positions").is_none()));
}

#[tokio::test]
async fn public_replay_stream_starts_from_requested_frame() {
    let app = seeded_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/9839/replay/stream?from=70&speed=16")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    let snapshots = sse_payloads(&text, "snapshot");

    assert_eq!(snapshots[0]["cursor"]["t"], 60.0);
}

async fn seeded_router() -> Router {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_demo_session(&pool).await.unwrap();
    storage::seed_mvp_fixture(&pool).await.unwrap();
    router(AppState::new(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
    ))
}

async fn get_json(app: Router, uri: &str, expected_status: StatusCode) -> Value {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), expected_status);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn cached_bahrain_bundle_without_location(session_key: i64) -> Vec<RawEndpoint> {
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
        raw(session_key, "location", json!([])),
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

fn raw(session_key: i64, endpoint: &str, payload: Value) -> RawEndpoint {
    RawEndpoint {
        endpoint: endpoint.to_string(),
        session_key,
        payload,
    }
}

fn sse_payloads(text: &str, event_name: &str) -> Vec<Value> {
    text.split("\n\n")
        .filter_map(|block| {
            let mut event = None;
            let mut data = String::new();
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event: ") {
                    event = Some(value);
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data.push_str(value);
                }
            }
            (event == Some(event_name))
                .then(|| serde_json::from_str::<Value>(&data).ok())
                .flatten()
        })
        .collect()
}
