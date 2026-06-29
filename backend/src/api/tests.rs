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
            auth: crate::connectors::openf1_live::OpenF1Auth::None,
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

// --- settings routes -------------------------------------------------------------
//
