use super::*;
use crate::{connectors::openf1_historical::RawEndpoint, replay, storage};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode as HttpStatusCode},
    routing::get,
    Json,
};
use chrono::Duration as ChronoDuration;
use futures_util::StreamExt;
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
async fn cancelled_session_ingest_is_blocked_before_fastf1() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let meeting = crate::domain::Meeting {
        meeting_key: 2601,
        year: 2026,
        name: "Saudi Arabian Grand Prix".to_string(),
        country: "Saudi Arabia".to_string(),
        location: "Jeddah".to_string(),
    };
    let session = crate::domain::Session {
        session_key: 26001,
        meeting_key: meeting.meeting_key,
        year: meeting.year,
        name: "Race".to_string(),
        session_type: crate::domain::SessionType::Race,
        start_time: "2026-04-19T17:00:00Z".to_string(),
        end_time: "2026-04-19T19:00:00Z".to_string(),
        total_laps: 0,
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
                .uri("/api/sessions/26001/ingest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert_eq!(payload["session_key"], 26001);
    assert_eq!(payload["status"], "failed");
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("cancelled")));

    let readiness = storage::list_session_readiness(&pool, 2601)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        readiness.support_status,
        crate::domain::SessionSupportStatus::Cancelled
    );
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
async fn live_current_is_inactive_when_openf1_live_is_disabled() {
    let app = seeded_router_with_live_enabled(false).await;

    let payload = get_json(app, "/api/live/current", StatusCode::OK).await;

    assert_eq!(payload["availability"], "disabled");
    assert_eq!(payload["active"], false);
    assert_eq!(payload["session"], Value::Null);
    assert!(payload["message"]
        .as_str()
        .is_some_and(|message| message.contains("disabled")));
}

#[tokio::test]
async fn live_start_reports_bad_request_when_openf1_live_is_disabled() {
    let app = seeded_router_with_live_enabled(false).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/123/live/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("OpenF1 live mode is disabled")));
}

#[tokio::test]
async fn live_current_reports_error_when_openf1_discovery_fails() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: "http://127.0.0.1:9/v1/".parse().unwrap(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let payload = get_json(app, "/api/live/current", StatusCode::OK).await;

    assert_eq!(payload["availability"], "error");
    assert_eq!(payload["active"], false);
    assert!(payload["message"]
        .as_str()
        .is_some_and(|message| message.contains("OpenF1 live discovery failed")));
}

#[tokio::test]
async fn live_start_reports_bad_gateway_when_openf1_discovery_fails() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: "http://127.0.0.1:9/v1/".parse().unwrap(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/123/live/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("OpenF1 live request failed")));
}

#[tokio::test]
async fn live_current_reports_configuration_errors() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config_error(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: "https://api.openf1.org/v1/".parse().unwrap(),
            token: None,
            auth_header: "authorization".to_string(),
        },
        "INTERVAL_OPENF1_LIVE_BASE_URL is invalid",
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let payload = get_json(app, "/api/live/current", StatusCode::OK).await;

    assert_eq!(payload["availability"], "error");
    assert_eq!(payload["active"], false);
    assert!(payload["message"].as_str().is_some_and(|message| {
        message.contains("OpenF1 live configuration error")
            && message.contains("INTERVAL_OPENF1_LIVE_BASE_URL")
    }));
}

#[tokio::test]
async fn live_start_reports_bad_gateway_when_openf1_live_is_misconfigured() {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config_error(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: "https://api.openf1.org/v1/".parse().unwrap(),
            token: None,
            auth_header: "authorization".to_string(),
        },
        "INTERVAL_OPENF1_LIVE_BASE_URL is invalid",
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/123/live/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("OpenF1 live configuration error")));
}

#[tokio::test]
async fn live_start_reports_bad_gateway_when_openf1_live_auth_header_is_invalid() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: Some("token".to_string()),
            auth_header: "bad header".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{}/live/start",
                    live_mock.session_key
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("invalid OpenF1 live auth header")));
}

#[tokio::test]
async fn live_current_is_inactive_when_session_window_is_future() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        start_offset: ChronoDuration::hours(2),
        end_offset: ChronoDuration::hours(4),
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let current = get_json(app.clone(), "/api/live/current", StatusCode::OK).await;

    assert_eq!(current["availability"], "inactive");
    assert_eq!(current["active"], false);
    assert_eq!(current["session"], Value::Null);
    assert_eq!(
        current["next_session"]["session_key"],
        live_mock.session_key
    );
    assert_eq!(current["next_meeting"]["name"], "Test Live Grand Prix");
}

#[tokio::test]
async fn live_current_is_inactive_during_pre_session_padding() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        start_offset: ChronoDuration::minutes(15),
        end_offset: ChronoDuration::hours(2),
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let current = get_json(app.clone(), "/api/live/current", StatusCode::OK).await;

    assert_eq!(current["availability"], "inactive");
    assert_eq!(current["active"], false);
    assert_eq!(current["session"], Value::Null);
    assert_eq!(
        current["next_session"]["session_key"],
        live_mock.session_key
    );
}

#[tokio::test]
async fn live_current_detects_active_sprint_sessions() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        session_name: "Sprint",
        session_type: "Sprint",
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let current = get_json(app.clone(), "/api/live/current", StatusCode::OK).await;

    assert_eq!(current["availability"], "active");
    assert_eq!(current["active"], true);
    assert_eq!(current["session"]["session_type"], "sprint");
    assert_eq!(current["session"]["session_key"], live_mock.session_key);

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["source"], "openf1_live");

    let metadata = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/metadata", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(metadata["session"]["session_type"], "sprint");

    let snapshot = get_json(
        app,
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["cursor"]["session_key"], live_mock.session_key);
    assert!(
        !snapshot["timing"]["rows"].as_array().unwrap().is_empty(),
        "snapshot payload: {snapshot}"
    );
}

#[tokio::test]
async fn openf1_live_current_start_snapshot_and_geometry_use_mocked_live_rows() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let current = get_json(app.clone(), "/api/live/current", StatusCode::OK).await;
    assert_eq!(current["availability"], "active");
    assert_eq!(current["active"], true);
    assert_eq!(current["session"]["session_key"], live_mock.session_key);
    assert_eq!(current["status"], Value::Null);

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);
    assert_eq!(status["source"], "openf1_live");
    assert!(status["channels"]
        .as_array()
        .is_some_and(|channels| channels
            .iter()
            .any(|channel| { channel["endpoint"] == "location" && channel["state"] == "fresh" })));

    let metadata = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/metadata", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(metadata["contract_version"], "replay.v1");
    assert_eq!(metadata["session"]["session_key"], live_mock.session_key);
    assert_eq!(metadata["data_sources"][0]["name"], "openf1_live");

    let snapshot = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["contract_version"], "replay.v1");
    assert_eq!(snapshot["cursor"]["session_key"], live_mock.session_key);
    assert_eq!(snapshot["timing"]["rows"][0]["driver"]["code"], "VER");
    assert_eq!(snapshot["timing"]["rows"][0]["gap_to_leader"], Value::Null);
    assert!(snapshot["cursor"]["t"].as_f64().is_some_and(|t| t > 0.0));

    let geometry = get_json(
        app.clone(),
        &format!(
            "/api/sessions/{}/live/track/geometry",
            live_mock.session_key
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(geometry["session_key"], live_mock.session_key);
    assert!(matches!(
        geometry["quality"].as_str(),
        Some("ready") | Some("schematic")
    ));

    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let _ = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    let refreshed_status = get_json(
        app,
        &format!("/api/sessions/{}/live/status", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(refreshed_status["started_at"], status["started_at"]);
    let counts = live_mock.call_counts();
    assert_eq!(counts.get("drivers").copied().unwrap_or_default(), 1);
    assert!(counts.get("location").copied().unwrap_or_default() >= 2);
}

#[tokio::test]
async fn openf1_live_current_returns_running_session_when_discovery_later_fails() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        fail_after_first_endpoints: vec!["meetings", "sessions"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;

    let current = get_json(app.clone(), "/api/live/current", StatusCode::OK).await;

    assert_eq!(current["availability"], "active");
    assert_eq!(current["active"], true);
    assert_eq!(current["session"]["session_key"], live_mock.session_key);
    assert_eq!(current["status"]["active"], true);

    let restarted = post_json(
        app,
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;

    assert_eq!(restarted["active"], true);
    assert_eq!(restarted["session_key"], live_mock.session_key);
    let counts = live_mock.call_counts();
    assert_eq!(counts.get("meetings").copied().unwrap_or_default(), 1);
    assert_eq!(counts.get("sessions").copied().unwrap_or_default(), 1);
}

#[tokio::test]
async fn openf1_live_post_session_padding_keeps_snapshot_clock_advancing() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        start_offset: -ChronoDuration::minutes(70),
        end_offset: -ChronoDuration::minutes(10),
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let current = get_json(app.clone(), "/api/live/current", StatusCode::OK).await;
    assert_eq!(current["availability"], "active");

    post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    let metadata = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/metadata", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    let snapshot = get_json(
        app,
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;

    let max_t = metadata["max_t"].as_f64().unwrap();
    let t = snapshot["cursor"]["t"].as_f64().unwrap();
    assert!(max_t > 3_600.0);
    // The clock keeps advancing past the scheduled end, lagged behind the feed.
    assert!(t > 3_600.0);
    assert!(t <= max_t);
}

#[tokio::test]
async fn openf1_live_start_rejects_stored_session_that_is_not_active() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let stored_meeting = crate::domain::Meeting {
        meeting_key: 91_000,
        year: chrono::Datelike::year(&chrono::Utc::now()),
        name: "Stored Grand Prix".to_string(),
        country: "Storedland".to_string(),
        location: "Stored Circuit".to_string(),
    };
    let stored_session = crate::domain::Session {
        session_key: 91_001,
        meeting_key: stored_meeting.meeting_key,
        year: stored_meeting.year,
        name: "Race".to_string(),
        session_type: crate::domain::SessionType::Race,
        start_time: "2026-01-01T13:00:00Z".to_string(),
        end_time: "2026-01-01T15:00:00Z".to_string(),
        total_laps: 50,
    };
    storage::upsert_meetings(&pool, &[stored_meeting])
        .await
        .unwrap();
    storage::upsert_sessions(&pool, &[stored_session.clone()])
        .await
        .unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{}/live/start",
                    stored_session.session_key
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice::<Value>(&body).unwrap();
    assert!(payload["error"]
        .as_str()
        .is_some_and(|message| message.contains("active OpenF1 live session")));
}

#[tokio::test]
async fn openf1_live_degrades_when_optional_channels_are_missing() {
    let live_mock = spawn_openf1_live_mock_with_missing(&["intervals", "race_control"]).await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);
    assert!(status["channels"]
        .as_array()
        .is_some_and(|channels| channels.iter().any(|channel| {
            channel["endpoint"] == "intervals" && channel["state"] == "missing"
        })));

    let snapshot = get_json(
        app,
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["contract_version"], "replay.v1");
    assert_eq!(snapshot["cursor"]["session_key"], live_mock.session_key);
    assert!(snapshot["timing"]["rows"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
}

#[tokio::test]
async fn openf1_live_reports_initial_endpoint_failures_in_channel_health() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        failed_endpoints: vec!["weather"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;

    let weather = status["channels"]
        .as_array()
        .and_then(|channels| {
            channels
                .iter()
                .find(|channel| channel["endpoint"] == "weather")
        })
        .expect("weather health should be present");
    assert_eq!(weather["state"], "failed");
    assert!(weather["last_error"]
        .as_str()
        .is_some_and(|error| error.contains("OpenF1 live request failed")));

    let snapshot = get_json(
        app,
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["contract_version"], "replay.v1");
}

#[tokio::test]
async fn openf1_live_keeps_last_snapshot_when_refresh_endpoint_fails() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        fail_after_first_endpoints: vec!["location"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(650)).await;

    let snapshot = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["contract_version"], "replay.v1");
    assert_eq!(snapshot["cursor"]["session_key"], live_mock.session_key);
    assert!(snapshot["track"]["positions"]
        .as_array()
        .is_some_and(|positions| !positions.is_empty()));

    let status = get_json(
        app,
        &format!("/api/sessions/{}/live/status", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    let location = status["channels"]
        .as_array()
        .and_then(|channels| {
            channels
                .iter()
                .find(|channel| channel["endpoint"] == "location")
        })
        .expect("location health should be present");
    assert_eq!(location["state"], "cached");
    assert!(location["last_error"]
        .as_str()
        .is_some_and(|error| error.contains("OpenF1 live request failed")));
}

#[tokio::test]
async fn openf1_live_survives_malformed_rows_in_refresh_payload() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        malformed_after_first_endpoints: vec!["location"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(650)).await;

    let snapshot = get_json(
        app.clone(),
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["contract_version"], "replay.v1");
    assert_eq!(snapshot["cursor"]["session_key"], live_mock.session_key);
    assert!(snapshot["track"]["positions"]
        .as_array()
        .is_some_and(|positions| !positions.is_empty()));

    let status = get_json(
        app,
        &format!("/api/sessions/{}/live/status", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    // Malformed rows are skipped per-row instead of failing the refresh, so
    // the session stays healthy and no refresh-failure channel appears.
    let channels = status["channels"].as_array().unwrap();
    assert!(!channels
        .iter()
        .any(|channel| channel["endpoint"] == "refresh"));
    assert!(status["active"].as_bool().unwrap());
}

#[tokio::test]
async fn openf1_live_start_rejects_empty_initial_race_state() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        missing_endpoints: vec!["drivers"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let payload = post_json(
        app,
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;

    assert!(payload["error"]
        .as_str()
        .is_some_and(|error| error.contains("no driver data")));
}

#[tokio::test]
async fn openf1_live_start_reports_bad_gateway_when_required_initial_endpoint_fails() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        failed_endpoints: vec!["drivers"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let payload = post_json(
        app,
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::BAD_GATEWAY,
    )
    .await;

    assert!(payload["error"]
        .as_str()
        .is_some_and(|error| error.contains("OpenF1 live request failed")));
}

#[tokio::test]
async fn openf1_live_start_rejects_initial_state_without_timing_data() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        missing_endpoints: vec![
            "laps",
            "intervals",
            "position",
            "location",
            "session_result",
        ],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let payload = post_json(
        app,
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;

    assert!(payload["error"]
        .as_str()
        .is_some_and(|error| error.contains("no timing/location data")));
}

#[tokio::test]
async fn openf1_live_reuses_cached_matching_track_geometry() {
    let live_mock = spawn_openf1_live_mock_with_missing(&["location"]).await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let historical_meeting = crate::domain::Meeting {
        meeting_key: 77_000,
        year: 2025,
        name: "Test Live Grand Prix".to_string(),
        country: "Testland".to_string(),
        location: "Test Circuit".to_string(),
    };
    let historical_session = crate::domain::Session {
        session_key: 77_001,
        meeting_key: historical_meeting.meeting_key,
        year: historical_meeting.year,
        name: "Race".to_string(),
        session_type: crate::domain::SessionType::Race,
        start_time: "2025-06-01T13:00:00Z".to_string(),
        end_time: "2025-06-01T15:00:00Z".to_string(),
        total_laps: 50,
    };
    storage::upsert_meetings(&pool, &[historical_meeting])
        .await
        .unwrap();
    storage::upsert_sessions(&pool, &[historical_session.clone()])
        .await
        .unwrap();
    storage::replace_track_geometry(&pool, &test_ready_geometry(historical_session.session_key))
        .await
        .unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);

    let geometry = get_json(
        app.clone(),
        &format!(
            "/api/sessions/{}/live/track/geometry",
            live_mock.session_key
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(geometry["session_key"], live_mock.session_key);
    assert_eq!(geometry["source"], "fast_f1_telemetry");
    assert_eq!(geometry["quality"], "ready");

    let snapshot = get_json(
        app,
        &format!("/api/sessions/{}/live/snapshot", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["track"]["map_mode"], "gps");
}

#[tokio::test]
async fn openf1_live_events_endpoint_returns_current_live_timeline() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);

    let events = get_json(
        app,
        &format!("/api/sessions/{}/live/events", live_mock.session_key),
        StatusCode::OK,
    )
    .await;

    assert_eq!(events["contract_version"], "replay.v1");
    assert!(events["events"].as_array().is_some_and(|items| {
        items.iter().any(|event| {
            event["source"] == "open_f1"
                && event["kind"] == "race_control"
                && event["message"] == "GREEN LIGHT"
        })
    }));
}

#[tokio::test]
async fn openf1_live_stream_replays_current_events_for_reconnect_recovery() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);

    let text = get_sse_prefix(
        app,
        &format!("/api/sessions/{}/live/stream", live_mock.session_key),
        |text| {
            text.contains("event: metadata")
                && text.contains("event: snapshot")
                && text.contains("event: event")
        },
    )
    .await;

    let metadata = sse_payloads(&text, "metadata");
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0]["contract_version"], "replay.v1");
    assert_eq!(metadata[0]["session"]["session_key"], live_mock.session_key);
    assert_eq!(metadata[0]["data_sources"][0]["name"], "openf1_live");
    assert_eq!(
        metadata[0]["endpoints"]["events_endpoint"],
        format!("/api/sessions/{}/live/events", live_mock.session_key)
    );

    let snapshots = sse_payloads(&text, "snapshot");
    assert!(!snapshots.is_empty());
    assert_eq!(snapshots[0]["contract_version"], "replay.v1");
    assert_eq!(snapshots[0]["cursor"]["session_key"], live_mock.session_key);
    assert!(snapshots[0]["timing"]["rows"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));

    assert!(sse_payloads(&text, "event")
        .iter()
        .any(|event| event["message"] == "GREEN LIGHT"));
}

#[tokio::test]
async fn concurrent_live_snapshot_readers_share_one_upstream_refresh() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));
    let key = live_mock.session_key;
    post_json(
        app.clone(),
        &format!("/api/sessions/{key}/live/start"),
        StatusCode::OK,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(550)).await;
    let before = live_mock.call_counts();

    let path = format!("/api/sessions/{key}/live/snapshot");
    let (first, second) = tokio::join!(
        get_json(app.clone(), &path, StatusCode::OK),
        get_json(app, &path, StatusCode::OK)
    );
    assert_eq!(first["cursor"]["session_key"], key);
    assert_eq!(second["cursor"]["session_key"], key);

    let after = live_mock.call_counts();
    assert_eq!(after["position"] - before["position"], 1);
    assert_eq!(after["location"] - before["location"], 1);
}

#[tokio::test]
async fn stopping_during_refresh_does_not_resurrect_live_session() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        slow_after_first_endpoints: vec!["location"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));
    let key = live_mock.session_key;
    post_json(
        app.clone(),
        &format!("/api/sessions/{key}/live/start"),
        StatusCode::OK,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(550)).await;

    let snapshot_app = app.clone();
    let snapshot_path = format!("/api/sessions/{key}/live/snapshot");
    let refreshing =
        tokio::spawn(
            async move { get_json(snapshot_app, &snapshot_path, StatusCode::NOT_FOUND).await },
        );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let stopped = post_json(
        app.clone(),
        &format!("/api/sessions/{key}/live/stop"),
        StatusCode::OK,
    )
    .await;

    assert!(!stopped["active"].as_bool().unwrap());
    refreshing.await.unwrap();
    let status = get_json(
        app,
        &format!("/api/sessions/{key}/live/status"),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], false);
}

#[tokio::test]
async fn openf1_live_stream_emits_late_arriving_event_rows_once() {
    let live_mock = spawn_openf1_live_mock_with_config(LiveMockConfig {
        delayed_endpoints: vec!["race_control"],
        ..LiveMockConfig::default()
    })
    .await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);

    let text = get_sse_prefix(
        app,
        &format!("/api/sessions/{}/live/stream", live_mock.session_key),
        |text| text.contains("event: event"),
    )
    .await;

    let events = sse_payloads(&text, "event");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "race_control");
    assert_eq!(events[0]["message"], "GREEN LIGHT");
}

#[tokio::test]
async fn openf1_live_stream_emits_end_when_session_stops() {
    let live_mock = spawn_openf1_live_mock().await;
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled: true,
            base_url: live_mock.base_url.clone(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    let app = router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ));

    let status = post_json(
        app.clone(),
        &format!("/api/sessions/{}/live/start", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/sessions/{}/live/stream",
                    live_mock.session_key
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let stopped = post_json(
        app,
        &format!("/api/sessions/{}/live/stop", live_mock.session_key),
        StatusCode::OK,
    )
    .await;
    assert_eq!(stopped["active"], false);

    let text = read_sse_prefix(response, |text| {
        text.contains("event: metadata") && text.contains("event: end")
    })
    .await;
    assert!(text.contains("data: live session ended"));
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

#[tokio::test]
async fn live_simulation_start_status_snapshot_and_stop_use_cached_replay() {
    let app = seeded_router().await;

    let start = post_json(
        app.clone(),
        "/api/sessions/9839/live-simulation/start",
        StatusCode::OK,
    )
    .await;
    assert_eq!(start["session_key"], 9839);
    assert_eq!(start["active"], true);
    assert_eq!(start["source"], "live_simulation");

    let status = get_json(
        app.clone(),
        "/api/sessions/9839/live-simulation/status",
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["active"], true);

    let snapshot = get_json(
        app.clone(),
        "/api/sessions/9839/live-simulation/snapshot",
        StatusCode::OK,
    )
    .await;
    assert_eq!(snapshot["contract_version"], "replay.v1");
    assert_eq!(snapshot["cursor"]["session_key"], 9839);

    let stopped = post_json(
        app,
        "/api/sessions/9839/live-simulation/stop",
        StatusCode::OK,
    )
    .await;
    assert_eq!(stopped["active"], false);
}

#[tokio::test]
async fn live_simulation_requires_cached_replay() {
    let app = seeded_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/9472/live-simulation/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn live_simulation_stream_emits_metadata_snapshots_and_end() {
    let app = seeded_router().await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/9839/live-simulation/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/9839/live-simulation/stream?speed=1000")
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
    assert!(text.contains("event: end"));
    assert!(text.contains("\"name\":\"live_simulation\""));
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

async fn seeded_router_with_live_enabled(enabled: bool) -> Router {
    let pool = storage::connect("sqlite::memory:").await.unwrap();
    storage::migrate(&pool).await.unwrap();
    storage::seed_demo_session(&pool).await.unwrap();
    storage::seed_mvp_fixture(&pool).await.unwrap();
    let live_client = crate::connectors::openf1_live::OpenF1LiveClient::with_config(
        crate::connectors::openf1_live::OpenF1LiveConfig {
            enabled,
            base_url: "http://127.0.0.1:9/v1/".parse().unwrap(),
            token: None,
            auth_header: "authorization".to_string(),
        },
    );
    router(AppState::new_with_live(
        pool,
        crate::connectors::openf1_historical::HistoricalClient::default(),
        live_client,
    ))
}

struct LiveMock {
    base_url: url::Url,
    session_key: i64,
    calls: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<&'static str, usize>>>,
}

impl LiveMock {
    fn call_counts(&self) -> std::collections::HashMap<&'static str, usize> {
        self.calls.lock().unwrap().clone()
    }
}

async fn spawn_openf1_live_mock() -> LiveMock {
    spawn_openf1_live_mock_with_missing(&[]).await
}

async fn spawn_openf1_live_mock_with_missing(missing_endpoints: &[&'static str]) -> LiveMock {
    spawn_openf1_live_mock_with_config(LiveMockConfig {
        missing_endpoints: missing_endpoints.to_vec(),
        ..LiveMockConfig::default()
    })
    .await
}

#[derive(Clone)]
struct LiveMockConfig {
    start_offset: ChronoDuration,
    end_offset: ChronoDuration,
    session_name: &'static str,
    session_type: &'static str,
    missing_endpoints: Vec<&'static str>,
    failed_endpoints: Vec<&'static str>,
    fail_after_first_endpoints: Vec<&'static str>,
    malformed_after_first_endpoints: Vec<&'static str>,
    delayed_endpoints: Vec<&'static str>,
    slow_after_first_endpoints: Vec<&'static str>,
}

impl Default for LiveMockConfig {
    fn default() -> Self {
        Self {
            start_offset: -ChronoDuration::minutes(10),
            end_offset: ChronoDuration::minutes(90),
            session_name: "Race",
            session_type: "Race",
            missing_endpoints: vec![],
            failed_endpoints: vec![],
            fail_after_first_endpoints: vec![],
            malformed_after_first_endpoints: vec![],
            delayed_endpoints: vec![],
            slow_after_first_endpoints: vec![],
        }
    }
}

async fn spawn_openf1_live_mock_with_config(config: LiveMockConfig) -> LiveMock {
    let now = chrono::Utc::now();
    let start = now + config.start_offset;
    let end = now + config.end_offset;
    // Older than the live display delay so the lagged cursor covers the rows.
    let recent = now - ChronoDuration::seconds(12);
    let session_key = 88_001_i64;
    let meeting_key = 88_000_i64;
    let calls = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let payload = std::sync::Arc::new(LiveMockPayload {
        session_key,
        meeting_key,
        year: chrono::Datelike::year(&now),
        start: start.to_rfc3339(),
        end: end.to_rfc3339(),
        recent: recent.to_rfc3339(),
        session_name: config.session_name,
        session_type: config.session_type,
        calls: calls.clone(),
        missing_endpoints: config.missing_endpoints.iter().copied().collect(),
        failed_endpoints: config.failed_endpoints.iter().copied().collect(),
        fail_after_first_endpoints: config.fail_after_first_endpoints.iter().copied().collect(),
        malformed_after_first_endpoints: config
            .malformed_after_first_endpoints
            .iter()
            .copied()
            .collect(),
        delayed_endpoints: config.delayed_endpoints.iter().copied().collect(),
        slow_after_first_endpoints: config.slow_after_first_endpoints.iter().copied().collect(),
    });
    let app = Router::new()
        .route("/v1/meetings", get(mock_meetings))
        .route("/v1/sessions", get(mock_sessions))
        .route("/v1/drivers", get(mock_drivers))
        .route("/v1/laps", get(mock_laps))
        .route("/v1/intervals", get(mock_intervals))
        .route("/v1/position", get(mock_position))
        .route("/v1/location", get(mock_location))
        .route("/v1/pit", get(mock_empty))
        .route("/v1/race_control", get(mock_race_control))
        .route("/v1/stints", get(mock_stints))
        .route("/v1/weather", get(mock_weather))
        .route("/v1/session_result", get(mock_session_result))
        .with_state(payload);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    LiveMock {
        base_url: format!("http://{addr}/v1/").parse().unwrap(),
        session_key,
        calls,
    }
}

#[derive(Clone)]
struct LiveMockPayload {
    session_key: i64,
    meeting_key: i64,
    year: i32,
    start: String,
    end: String,
    recent: String,
    session_name: &'static str,
    session_type: &'static str,
    calls: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<&'static str, usize>>>,
    missing_endpoints: std::collections::HashSet<&'static str>,
    failed_endpoints: std::collections::HashSet<&'static str>,
    fail_after_first_endpoints: std::collections::HashSet<&'static str>,
    malformed_after_first_endpoints: std::collections::HashSet<&'static str>,
    delayed_endpoints: std::collections::HashSet<&'static str>,
    slow_after_first_endpoints: std::collections::HashSet<&'static str>,
}

impl LiveMockPayload {
    fn record(&self, endpoint: &'static str) {
        *self.calls.lock().unwrap().entry(endpoint).or_default() += 1;
    }

    fn call_count(&self, endpoint: &'static str) -> usize {
        *self.calls.lock().unwrap().get(endpoint).unwrap_or(&0)
    }

    fn is_missing(&self, endpoint: &'static str) -> bool {
        self.missing_endpoints.contains(endpoint)
    }

    fn is_initially_delayed(&self, endpoint: &'static str) -> bool {
        self.delayed_endpoints.contains(endpoint) && self.call_count(endpoint) <= 1
    }

    fn is_malformed_after_first(&self, endpoint: &'static str) -> bool {
        self.malformed_after_first_endpoints.contains(endpoint) && self.call_count(endpoint) > 1
    }

    async fn maybe_delay_after_first(&self, endpoint: &'static str) {
        if self.slow_after_first_endpoints.contains(endpoint) && self.call_count(endpoint) > 1 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    fn maybe_fail(&self, endpoint: &'static str) -> Result<(), HttpStatusCode> {
        if self.failed_endpoints.contains(endpoint)
            || (self.fail_after_first_endpoints.contains(endpoint) && self.call_count(endpoint) > 1)
        {
            Err(HttpStatusCode::INTERNAL_SERVER_ERROR)
        } else {
            Ok(())
        }
    }
}

async fn mock_meetings(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("meetings");
    payload.maybe_fail("meetings")?;
    Ok(Json(json!([{
        "meeting_key": payload.meeting_key,
        "meeting_name": "Test Live Grand Prix",
        "country_name": "Testland",
        "location": "Test Circuit",
        "year": payload.year
    }])))
}

async fn mock_sessions(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("sessions");
    payload.maybe_fail("sessions")?;
    Ok(Json(json!([{
        "session_key": payload.session_key,
        "meeting_key": payload.meeting_key,
        "session_name": payload.session_name,
        "session_type": payload.session_type,
        "date_start": payload.start,
        "date_end": payload.end,
        "year": payload.year
    }])))
}

async fn mock_drivers(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("drivers");
    payload.maybe_fail("drivers")?;
    if payload.is_missing("drivers") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "driver_number": 1,
        "full_name": "Max Verstappen",
        "name_acronym": "VER",
        "team_colour": "3671C6",
        "team_name": "Red Bull Racing"
    }])))
}

async fn mock_laps(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("laps");
    payload.maybe_fail("laps")?;
    if payload.is_missing("laps") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "driver_number": 1,
        "lap_number": 1,
        "date_start": payload.start,
        "lap_duration": 90.0,
        "duration_sector_1": 29.0,
        "duration_sector_2": 31.0,
        "duration_sector_3": 30.0
    }])))
}

async fn mock_intervals(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("intervals");
    payload.maybe_fail("intervals")?;
    if payload.is_missing("intervals") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "date": payload.recent,
        "driver_number": 1,
        "gap_to_leader": null,
        "interval": null
    }])))
}

async fn mock_position(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("position");
    payload.maybe_fail("position")?;
    if payload.is_missing("position") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "date": payload.recent,
        "driver_number": 1,
        "position": 1
    }])))
}

async fn mock_location(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("location");
    payload.maybe_delay_after_first("location").await;
    payload.maybe_fail("location")?;
    if payload.is_missing("location") {
        return Ok(Json(json!([])));
    }
    if payload.is_malformed_after_first("location") {
        return Ok(Json(json!([{
            "date": payload.recent,
            "driver_number": 1,
            "x": "not-a-number",
            "y": 50.0,
            "z": 0.0
        }])));
    }
    Ok(Json(json!([{
        "date": payload.recent,
        "driver_number": 1,
        "x": 100.0,
        "y": 50.0,
        "z": 0.0
    }])))
}

async fn mock_race_control(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("race_control");
    payload.maybe_fail("race_control")?;
    if payload.is_missing("race_control") || payload.is_initially_delayed("race_control") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "date": payload.recent,
        "category": "Flag",
        "message": "GREEN LIGHT",
        "flag": "GREEN",
        "scope": "Track"
    }])))
}

async fn mock_stints(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("stints");
    payload.maybe_fail("stints")?;
    if payload.is_missing("stints") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "driver_number": 1,
        "stint_number": 1,
        "compound": "MEDIUM",
        "lap_start": 1,
        "lap_end": null,
        "tyre_age_at_start": 0
    }])))
}

async fn mock_weather(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("weather");
    payload.maybe_fail("weather")?;
    if payload.is_missing("weather") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "date": payload.recent,
        "air_temperature": 22.0,
        "track_temperature": 34.0,
        "humidity": 45.0,
        "rainfall": 0.0,
        "wind_direction": 180,
        "wind_speed": 2.5
    }])))
}

async fn mock_session_result(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("session_result");
    payload.maybe_fail("session_result")?;
    if payload.is_missing("session_result") {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!([{
        "driver_number": 1,
        "position": 1,
        "dnf": false,
        "dns": false,
        "dsq": false
    }])))
}

async fn mock_empty(
    axum::extract::State(payload): axum::extract::State<std::sync::Arc<LiveMockPayload>>,
) -> Result<Json<Value>, HttpStatusCode> {
    payload.record("pit");
    payload.maybe_fail("pit")?;
    Ok(Json(json!([])))
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

async fn post_json(app: Router, uri: &str, expected_status: StatusCode) -> Value {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), expected_status);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn get_sse_prefix(app: Router, uri: &str, done: impl Fn(&str) -> bool) -> String {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    read_sse_prefix(response, done).await
}

async fn read_sse_prefix(
    response: axum::response::Response,
    done: impl Fn(&str) -> bool,
) -> String {
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.into_body().into_data_stream();
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        tokio::select! {
            chunk = stream.next() => {
                let chunk = chunk.expect("live stream should not end before expected events").unwrap();
                text.push_str(std::str::from_utf8(&chunk).unwrap());
                if done(&text) {
                    return text;
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                panic!("timed out waiting for SSE events; received:\n{text}");
            }
        }
    }
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

fn test_ready_geometry(session_key: i64) -> crate::domain::TrackGeometry {
    crate::domain::TrackGeometry {
        contract_version: crate::domain::REPLAY_CONTRACT_VERSION.to_string(),
        session_key,
        bounds: crate::domain::TrackBounds {
            min_x: 0.0,
            max_x: 100.0,
            min_y: 0.0,
            max_y: 100.0,
        },
        centerline: vec![
            track_point(0.0, 0.0, 0.0, 0.0),
            track_point(100.0, 0.0, 100.0, 0.25),
            track_point(100.0, 100.0, 200.0, 0.5),
            track_point(0.0, 100.0, 300.0, 0.75),
            track_point(0.0, 0.0, 400.0, 1.0),
        ],
        inner_edge: vec![],
        outer_edge: vec![],
        source: crate::domain::TrackGeometrySource::FastF1Telemetry,
        quality: crate::domain::TrackGeometryQuality::Ready,
        map_mode: crate::domain::MapMode::Gps,
        circuit_length: Some(400.0),
        generated_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn track_point(
    x: f64,
    y: f64,
    cumulative_distance: f64,
    relative_distance: f64,
) -> crate::domain::TrackPoint {
    crate::domain::TrackPoint {
        x,
        y,
        z: None,
        cumulative_distance,
        relative_distance,
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
