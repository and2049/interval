use crate::{
    connectors::{
        openf1_historical::RawEndpoint,
        openf1_live::{live_endpoint_cadence_seconds, OpenF1LiveClient},
    },
    domain::{
        DataSource, EndpointLinks, LiveAvailability, LiveChannelHealth, LiveChannelState,
        LiveCurrentResponse, LiveSessionStatus, Meeting, ReplayEvent, ReplayMetadata,
        ReplaySnapshot, Session, TrackGeometry, TrackGeometrySource,
    },
    normalization::{self, RaceDataSource},
    replay, storage,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures_util::future::join_all;
use serde_json::Value;
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;

const LIVE_FRAME_STEP_SECONDS: f64 = 0.5;
const LIVE_WINDOW_PADDING_MINUTES: i64 = 30;
const LIVE_INCREMENTAL_ROW_OVERLAP_SECONDS: i64 = 5;
const DEFAULT_LIVE_ENDPOINT_ROW_LIMIT: usize = 10_000;

#[derive(Clone)]
pub struct OpenF1LiveRegistry {
    client: OpenF1LiveClient,
    sessions: Arc<Mutex<HashMap<i64, OpenF1LiveSession>>>,
    refresh_gate: Arc<Mutex<()>>,
    lifecycle: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
struct OpenF1LiveSession {
    generation: u64,
    session: Session,
    meeting: Option<Meeting>,
    metadata: ReplayMetadata,
    snapshot: ReplaySnapshot,
    geometry: TrackGeometry,
    events: Vec<ReplayEvent>,
    raw_bundle: Vec<LiveCachedEndpoint>,
    started_at: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct LiveCachedEndpoint {
    raw: RawEndpoint,
    fetched_at: DateTime<Utc>,
    attempted_at: DateTime<Utc>,
    failure_count: u32,
    last_error: Option<String>,
}

impl OpenF1LiveRegistry {
    pub fn new(client: OpenF1LiveClient) -> Self {
        Self {
            client,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            refresh_gate: Arc::new(Mutex::new(())),
            lifecycle: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn current(&self, pool: &SqlitePool) -> anyhow::Result<LiveCurrentResponse> {
        if !self.client.enabled() {
            return Ok(LiveCurrentResponse {
                availability: LiveAvailability::Disabled,
                active: false,
                session: None,
                meeting: None,
                next_session: None,
                next_meeting: None,
                status: None,
                message: Some("OpenF1 live mode is disabled.".to_string()),
            });
        }

        if let Some(active) = self.current_active_session().await {
            return Ok(LiveCurrentResponse {
                availability: LiveAvailability::Active,
                active: true,
                session: Some(active.session.clone()),
                meeting: active.meeting.clone(),
                next_session: None,
                next_meeting: None,
                status: Some(status_from_session(&active, true)),
                message: None,
            });
        }

        let discovery = match self.client.current_and_next_session(Utc::now()).await {
            Ok(discovery) => discovery,
            Err(error) => {
                return Ok(LiveCurrentResponse {
                    availability: LiveAvailability::Error,
                    active: false,
                    session: None,
                    meeting: None,
                    next_session: None,
                    next_meeting: None,
                    status: None,
                    message: Some(format!("OpenF1 live discovery failed: {error}")),
                });
            }
        };
        let active = match discovery.current {
            Some(active) => active,
            None => {
                return Ok(LiveCurrentResponse {
                    availability: LiveAvailability::Inactive,
                    active: false,
                    session: None,
                    meeting: None,
                    next_session: discovery.next.as_ref().map(|active| active.session.clone()),
                    next_meeting: discovery.next.and_then(|active| active.meeting),
                    status: None,
                    message: Some("No active OpenF1 race or sprint session.".to_string()),
                });
            }
        };

        {
            if let Some(meeting) = &active.meeting {
                storage::upsert_meetings(pool, std::slice::from_ref(meeting)).await?;
            }
            storage::upsert_sessions(pool, std::slice::from_ref(&active.session)).await?;
        }

        Ok(LiveCurrentResponse {
            availability: LiveAvailability::Active,
            active: true,
            session: Some(active.session),
            meeting: active.meeting,
            next_session: None,
            next_meeting: None,
            status: None,
            message: None,
        })
    }

    pub async fn start(
        &self,
        pool: &SqlitePool,
        session_key: i64,
    ) -> anyhow::Result<LiveSessionStatus> {
        if let Some(active) = self.active_session(session_key).await {
            return Ok(status_from_session(&active, true));
        }

        let generation = self.lifecycle.fetch_add(1, Ordering::SeqCst) + 1;
        let _refresh = self.refresh_gate.lock().await;
        if self.lifecycle.load(Ordering::SeqCst) != generation {
            anyhow::bail!("OpenF1 live start was cancelled");
        }
        let session = live_session(pool, &self.client, session_key).await?;
        let meeting = storage::get_meeting(pool, session.meeting_key).await?;
        let live_session = self
            .refresh_live_session(pool, session, meeting, None, None, generation)
            .await?;
        if self.lifecycle.load(Ordering::SeqCst) != generation {
            anyhow::bail!("OpenF1 live start was cancelled");
        }
        let status = status_from_session(&live_session, true);
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|key, _| *key == session_key);
        sessions.insert(session_key, live_session);
        Ok(status)
    }

    pub async fn stop(&self, session_key: i64) -> LiveSessionStatus {
        self.lifecycle.fetch_add(1, Ordering::SeqCst);
        self.sessions
            .lock()
            .await
            .remove(&session_key)
            .map(|session| status_from_session(&session, false))
            .unwrap_or_else(|| inactive_status(session_key))
    }

    pub async fn status(&self, session_key: i64) -> LiveSessionStatus {
        self.active_session(session_key)
            .await
            .as_ref()
            .map(|session| status_from_session(session, true))
            .unwrap_or_else(|| inactive_status(session_key))
    }

    pub async fn snapshot(
        &self,
        pool: &SqlitePool,
        session_key: i64,
    ) -> anyhow::Result<Option<ReplaySnapshot>> {
        let _refresh = self.refresh_gate.lock().await;
        let Some(existing) = self.sessions.lock().await.get(&session_key).cloned() else {
            return Ok(None);
        };
        if live_session_window_closed(&existing.session, Utc::now()) {
            self.sessions.lock().await.remove(&session_key);
            return Ok(None);
        }
        if live_session_refresh_is_fresh(&existing, Utc::now()) {
            return Ok(Some(existing.snapshot));
        }
        let refreshed = match self
            .refresh_live_session(
                pool,
                existing.session.clone(),
                existing.meeting.clone(),
                Some(existing.raw_bundle.clone()),
                Some(existing.started_at.clone()),
                existing.generation,
            )
            .await
        {
            Ok(refreshed) => refreshed,
            Err(error) => live_session_with_refresh_error(existing, error),
        };
        let mut sessions = self.sessions.lock().await;
        let Some(current) = sessions.get(&session_key) else {
            return Ok(None);
        };
        if current.generation != refreshed.generation {
            return Ok(Some(current.snapshot.clone()));
        }
        let snapshot = refreshed.snapshot.clone();
        sessions.insert(session_key, refreshed);
        Ok(Some(snapshot))
    }

    pub async fn metadata(&self, session_key: i64) -> Option<ReplayMetadata> {
        self.active_session(session_key)
            .await
            .as_ref()
            .map(|session| session.metadata.clone())
    }

    pub async fn geometry(&self, session_key: i64) -> Option<TrackGeometry> {
        self.active_session(session_key)
            .await
            .as_ref()
            .map(|session| session.geometry.clone())
    }

    pub async fn events_since(
        &self,
        session_key: i64,
        previous_t: f64,
        t: f64,
    ) -> Vec<ReplayEvent> {
        self.active_session(session_key)
            .await
            .as_ref()
            .map(|session| {
                session
                    .events
                    .iter()
                    .filter(|event| event.t > previous_t && event.t <= t)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn events(&self, session_key: i64) -> Option<Vec<ReplayEvent>> {
        self.active_session(session_key)
            .await
            .as_ref()
            .map(|session| session.events.clone())
    }

    async fn active_session(&self, session_key: i64) -> Option<OpenF1LiveSession> {
        let mut sessions = self.sessions.lock().await;
        if sessions
            .get(&session_key)
            .is_some_and(|session| live_session_window_closed(&session.session, Utc::now()))
        {
            sessions.remove(&session_key);
            return None;
        }
        sessions.get(&session_key).cloned()
    }

    async fn current_active_session(&self) -> Option<OpenF1LiveSession> {
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, session| !live_session_window_closed(&session.session, Utc::now()));
        sessions.values().next().cloned()
    }

    async fn refresh_live_session(
        &self,
        pool: &SqlitePool,
        session: Session,
        meeting: Option<Meeting>,
        previous_bundle: Option<Vec<LiveCachedEndpoint>>,
        previous_started_at: Option<String>,
        generation: u64,
    ) -> anyhow::Result<OpenF1LiveSession> {
        let raw_bundle = self
            .fetch_cadenced_live_bundle(session.session_key, previous_bundle)
            .await?;
        let bundle: Vec<RawEndpoint> = raw_bundle
            .iter()
            .map(|endpoint| endpoint.raw.clone())
            .collect();
        let mut data = normalization::race_data_from_bundle(&bundle, &session)?;
        data.source = RaceDataSource::OpenF1Live;
        ensure_live_data_usable(&data, &raw_bundle)?;
        let events = replay::events::generate_events(&data);
        let geometry = live_track_geometry(pool, &session, meeting.as_ref(), &data).await?;
        let (snapshot_t, max_t) = live_snapshot_time_bounds(&session, Utc::now());
        let index = replay::indexed_data::ReplayDataIndex::new(&data);
        let frame_index = (snapshot_t / LIVE_FRAME_STEP_SECONDS).floor() as i64;
        let snapshot = replay::snapshot_builder::build_indexed_snapshot(
            &session,
            &index,
            &geometry,
            snapshot_t,
            frame_index,
        );
        let mut metadata = replay::metadata_builder::build_metadata(
            &session,
            &data,
            &geometry,
            max_t,
            LIVE_FRAME_STEP_SECONDS,
            (max_t / LIVE_FRAME_STEP_SECONDS).ceil() as i64,
        );
        metadata.meeting = meeting.clone();
        metadata.data_sources = vec![DataSource {
            name: "openf1_live".to_string(),
            mode: "live".to_string(),
        }];
        metadata.endpoints = live_endpoint_links(session.session_key);

        let timestamp = Utc::now().to_rfc3339();
        Ok(OpenF1LiveSession {
            generation,
            session,
            meeting,
            metadata,
            snapshot,
            geometry,
            events,
            raw_bundle,
            started_at: previous_started_at.unwrap_or_else(|| timestamp.clone()),
            updated_at: timestamp,
        })
    }

    async fn fetch_cadenced_live_bundle(
        &self,
        session_key: i64,
        previous_bundle: Option<Vec<LiveCachedEndpoint>>,
    ) -> anyhow::Result<Vec<LiveCachedEndpoint>> {
        if previous_bundle.is_none() {
            let endpoint_fetches = self.client.endpoint_specs().iter().map(|spec| {
                let client = self.client.clone();
                async move {
                    let fetched = client
                        .fetch_live_bundle_endpoint(session_key, spec.name)
                        .await;
                    let fetched_at = Utc::now();
                    match fetched {
                        Ok(raw) => LiveCachedEndpoint {
                            raw,
                            fetched_at,
                            attempted_at: fetched_at,
                            failure_count: 0,
                            last_error: None,
                        },
                        Err(error) => LiveCachedEndpoint {
                            raw: RawEndpoint {
                                endpoint: spec.name.to_string(),
                                session_key,
                                payload: Value::Array(vec![]),
                            },
                            fetched_at,
                            attempted_at: fetched_at,
                            failure_count: 1,
                            last_error: Some(error.to_string()),
                        },
                    }
                }
            });
            return Ok(join_all(endpoint_fetches).await);
        }

        let now = Utc::now();
        let mut previous_by_endpoint: HashMap<String, LiveCachedEndpoint> = previous_bundle
            .unwrap_or_default()
            .into_iter()
            .map(|endpoint| (endpoint.raw.endpoint.clone(), endpoint))
            .collect();
        let endpoint_fetches = self.client.endpoint_specs().iter().map(|spec| {
            let client = self.client.clone();
            let previous = previous_by_endpoint.remove(spec.name);
            let is_fresh = previous.as_ref().is_some_and(|endpoint| {
                now.signed_duration_since(endpoint.attempted_at)
                    < live_retry_delay(spec.cadence_ms, endpoint.failure_count)
            });

            async move {
                if is_fresh {
                    return previous.ok_or_else(|| anyhow::anyhow!("missing fresh live endpoint"));
                }

                let since = previous
                    .as_ref()
                    .map(|endpoint| live_incremental_since(endpoint, spec.incremental_field));
                let fetched = match since {
                    Some(since) => {
                        client
                            .fetch_live_bundle_endpoint_since(session_key, spec.name, since)
                            .await
                    }
                    None => {
                        client
                            .fetch_live_bundle_endpoint(session_key, spec.name)
                            .await
                    }
                };

                match fetched {
                    Ok(raw) => Ok(LiveCachedEndpoint {
                        raw: merge_live_payload(previous.map(|endpoint| endpoint.raw), raw),
                        fetched_at: Utc::now(),
                        attempted_at: Utc::now(),
                        failure_count: 0,
                        last_error: None,
                    }),
                    Err(error) => {
                        if let Some(mut endpoint) = previous {
                            endpoint.attempted_at = Utc::now();
                            endpoint.failure_count = endpoint.failure_count.saturating_add(1);
                            endpoint.last_error = Some(error.to_string());
                            Ok(endpoint)
                        } else {
                            Err(error.into())
                        }
                    }
                }
            }
        });

        join_all(endpoint_fetches).await.into_iter().collect()
    }
}

fn merge_live_payload(previous: Option<RawEndpoint>, mut fetched: RawEndpoint) -> RawEndpoint {
    let Some(previous) = previous else {
        return fetched;
    };
    let Value::Array(mut rows) = previous.payload else {
        return fetched;
    };
    let Value::Array(fetched_rows) = fetched.payload else {
        return fetched;
    };

    let mut indexes = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| live_row_key(row).map(|key| (key, index)))
        .collect::<HashMap<_, _>>();
    for row in fetched_rows {
        let Some(key) = live_row_key(&row) else {
            continue;
        };
        if let Some(index) = indexes.get(&key).copied() {
            rows[index] = row;
        } else {
            indexes.insert(key, rows.len());
            rows.push(row);
        }
    }
    trim_live_payload_rows(&fetched.endpoint, &mut rows);
    fetched.payload = Value::Array(rows);
    fetched
}

fn live_row_key(row: &Value) -> Option<String> {
    if let Some(date) = row.get("date").and_then(Value::as_str) {
        let driver = row
            .get("driver_number")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        return Some(format!("{driver}:{date}"));
    }
    serde_json::to_string(row).ok()
}

fn live_session_with_refresh_error(
    mut session: OpenF1LiveSession,
    error: anyhow::Error,
) -> OpenF1LiveSession {
    session.raw_bundle.push(LiveCachedEndpoint {
        raw: RawEndpoint {
            endpoint: "refresh".to_string(),
            session_key: session.session.session_key,
            payload: Value::Array(vec![]),
        },
        fetched_at: Utc::now(),
        attempted_at: Utc::now(),
        failure_count: 1,
        last_error: Some(format!("OpenF1 live refresh failed: {error}")),
    });
    session.updated_at = Utc::now().to_rfc3339();
    session
}

fn live_retry_delay(cadence_ms: i64, failure_count: u32) -> ChronoDuration {
    let multiplier = 1_i64 << failure_count.min(5);
    ChronoDuration::milliseconds((cadence_ms * multiplier).min(30_000))
}

fn live_session_refresh_is_fresh(session: &OpenF1LiveSession, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(&session.updated_at)
        .map(|updated| {
            now.signed_duration_since(updated.with_timezone(&Utc))
                < ChronoDuration::milliseconds((LIVE_FRAME_STEP_SECONDS * 1_000.0) as i64)
        })
        .unwrap_or(false)
}

fn live_incremental_since(
    endpoint: &LiveCachedEndpoint,
    incremental_field: Option<&str>,
) -> DateTime<Utc> {
    incremental_field
        .and_then(|field| latest_row_timestamp(&endpoint.raw.payload, field))
        .unwrap_or(endpoint.fetched_at)
        - ChronoDuration::seconds(LIVE_INCREMENTAL_ROW_OVERLAP_SECONDS)
}

fn latest_row_timestamp(payload: &Value, field: &str) -> Option<DateTime<Utc>> {
    payload
        .as_array()?
        .iter()
        .filter_map(|row| row.get(field)?.as_str())
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .max()
}

fn trim_live_payload_rows(endpoint: &str, rows: &mut Vec<Value>) {
    let limit = live_endpoint_row_limit(endpoint);
    if rows.len() > limit {
        rows.drain(..rows.len() - limit);
    }
}

fn live_endpoint_row_limit(endpoint: &str) -> usize {
    match endpoint {
        "location" => 20_000,
        "position" | "intervals" => 8_000,
        "laps" => 4_000,
        "pit" | "race_control" | "weather" => 1_000,
        _ => DEFAULT_LIVE_ENDPOINT_ROW_LIMIT,
    }
}

fn ensure_live_data_usable(
    data: &normalization::RaceData,
    bundle: &[LiveCachedEndpoint],
) -> anyhow::Result<()> {
    if data.drivers.is_empty() {
        if let Some(error) = live_endpoint_error(bundle, "drivers") {
            return Err(anyhow::anyhow!(error.to_string()));
        }
        return Err(anyhow::anyhow!(
            "OpenF1 live initial snapshot has no driver data"
        ));
    }

    if data.positions.is_empty()
        && data.intervals.is_empty()
        && data.laps.is_empty()
        && data.locations.is_empty()
        && data.session_results.is_empty()
    {
        for endpoint in [
            "position",
            "intervals",
            "laps",
            "location",
            "session_result",
        ] {
            if let Some(error) = live_endpoint_error(bundle, endpoint) {
                return Err(anyhow::anyhow!(error.to_string()));
            }
        }
        return Err(anyhow::anyhow!(
            "OpenF1 live initial snapshot has no timing/location data"
        ));
    }

    Ok(())
}

fn live_endpoint_error<'a>(bundle: &'a [LiveCachedEndpoint], endpoint: &str) -> Option<&'a str> {
    bundle
        .iter()
        .find(|cached| cached.raw.endpoint == endpoint)
        .and_then(|cached| cached.last_error.as_deref())
}

async fn live_track_geometry(
    pool: &SqlitePool,
    session: &Session,
    meeting: Option<&Meeting>,
    data: &normalization::RaceData,
) -> anyhow::Result<TrackGeometry> {
    if let Some(geometry) = storage::get_track_geometry(pool, session.session_key).await? {
        return Ok(geometry);
    }
    if let Some(meeting) = meeting {
        if let Some(geometry) =
            storage::get_reusable_track_geometry_for_meeting(pool, meeting, session.session_key)
                .await?
        {
            return Ok(geometry);
        }
    }

    Ok(replay::track_geometry_builder::build_track_geometry(
        session.session_key,
        if data.geometry_locations.is_empty() {
            &data.locations
        } else {
            &data.geometry_locations
        },
        TrackGeometrySource::OpenF1Location,
    ))
}

async fn live_session(
    pool: &SqlitePool,
    client: &OpenF1LiveClient,
    session_key: i64,
) -> anyhow::Result<Session> {
    if !client.enabled() {
        return Err(anyhow::anyhow!("OpenF1 live mode is disabled"));
    }
    let active = client
        .current_session(Utc::now())
        .await?
        .ok_or_else(|| anyhow::anyhow!("no active OpenF1 live session"))?;
    if active.session.session_key != session_key {
        return Err(anyhow::anyhow!(
            "requested session is not the active OpenF1 live session"
        ));
    }
    if let Some(meeting) = &active.meeting {
        storage::upsert_meetings(pool, std::slice::from_ref(meeting)).await?;
    }
    storage::upsert_sessions(pool, std::slice::from_ref(&active.session)).await?;
    Ok(active.session)
}

fn status_from_session(session: &OpenF1LiveSession, active: bool) -> LiveSessionStatus {
    LiveSessionStatus {
        session_key: session.session.session_key,
        active,
        current_t: Some(session.snapshot.cursor.t),
        max_t: Some(session.metadata.max_t),
        started_at: Some(session.started_at.clone()),
        updated_at: Some(session.updated_at.clone()),
        source: Some("openf1_live".to_string()),
        channels: live_channel_health(&session.raw_bundle, Utc::now()),
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

fn live_channel_health(
    bundle: &[LiveCachedEndpoint],
    now: DateTime<Utc>,
) -> Vec<LiveChannelHealth> {
    bundle
        .iter()
        .map(|endpoint| {
            let age_seconds = now
                .signed_duration_since(endpoint.fetched_at)
                .num_milliseconds() as f64
                / 1_000.0;
            let age_seconds = age_seconds.max(0.0);
            let rows = endpoint.raw.payload.as_array().map(Vec::len);
            let has_rows = rows.unwrap_or(0) > 0;
            let cadence_seconds = live_endpoint_cadence_seconds(&endpoint.raw.endpoint);
            let cached_seconds = (cadence_seconds * 3.0).max(10.0);
            let optional_empty = live_endpoint_allows_empty_payload(&endpoint.raw.endpoint);
            let state = if endpoint.last_error.is_some() && has_rows {
                if age_seconds <= cached_seconds {
                    LiveChannelState::Cached
                } else {
                    LiveChannelState::Stale
                }
            } else if endpoint.last_error.is_some() {
                LiveChannelState::Failed
            } else if rows == Some(0) && !optional_empty {
                LiveChannelState::Missing
            } else if age_seconds <= cadence_seconds.max(1.0) {
                LiveChannelState::Fresh
            } else if age_seconds <= cached_seconds {
                LiveChannelState::Cached
            } else {
                LiveChannelState::Stale
            };
            LiveChannelHealth {
                endpoint: endpoint.raw.endpoint.clone(),
                state,
                age_seconds: Some(age_seconds),
                rows,
                last_error: endpoint.last_error.clone(),
            }
        })
        .collect()
}

fn live_endpoint_allows_empty_payload(endpoint: &str) -> bool {
    matches!(endpoint, "pit" | "race_control" | "session_result")
}

fn live_endpoint_links(session_key: i64) -> EndpointLinks {
    EndpointLinks {
        snapshot_endpoint: format!("/api/sessions/{session_key}/live/snapshot"),
        stream_endpoint: format!("/api/sessions/{session_key}/live/stream"),
        events_endpoint: format!("/api/sessions/{session_key}/live/events"),
        track_geometry_endpoint: format!("/api/sessions/{session_key}/live/track/geometry"),
    }
}

fn t_since_session_start(session: &Session, now: DateTime<Utc>) -> f64 {
    let Ok(start) = DateTime::parse_from_rfc3339(&session.start_time) else {
        return 0.0;
    };
    now.signed_duration_since(start.with_timezone(&Utc))
        .num_milliseconds() as f64
        / 1_000.0
}

fn session_duration(session: &Session) -> Option<f64> {
    let start = DateTime::parse_from_rfc3339(&session.start_time)
        .ok()?
        .with_timezone(&Utc);
    let end = live_session_end_or_default(start, &session.end_time);
    Some(end.signed_duration_since(start).num_milliseconds() as f64 / 1_000.0)
}

fn live_snapshot_time_bounds(session: &Session, now: DateTime<Utc>) -> (f64, f64) {
    let now_t = t_since_session_start(session, now).max(0.0);
    let max_t = session_duration(session)
        .unwrap_or(now_t + LIVE_FRAME_STEP_SECONDS)
        .max(now_t);
    (now_t.min(max_t), max_t)
}

fn live_session_window_closed(session: &Session, now: DateTime<Utc>) -> bool {
    let Ok(start) = DateTime::parse_from_rfc3339(&session.start_time) else {
        return false;
    };
    let start = start.with_timezone(&Utc);
    let end = live_session_end_or_default(start, &session.end_time);
    now > end + ChronoDuration::minutes(LIVE_WINDOW_PADDING_MINUTES)
}

fn live_session_end_or_default(start: DateTime<Utc>, end_time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(end_time)
        .map(|value| value.with_timezone(&Utc))
        .ok()
        .filter(|end| *end > start)
        .unwrap_or_else(|| start + ChronoDuration::hours(3))
}

#[allow(dead_code)]
fn empty_bundle(session_key: i64) -> Vec<RawEndpoint> {
    [
        "drivers",
        "laps",
        "intervals",
        "position",
        "location",
        "pit",
        "race_control",
        "stints",
        "weather",
        "session_result",
    ]
    .into_iter()
    .map(|endpoint| RawEndpoint {
        endpoint: endpoint.to_string(),
        session_key,
        payload: Value::Array(vec![]),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionType;

    #[test]
    fn channel_health_marks_cached_payload_with_refresh_error_as_cached() {
        let endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "location".to_string(),
                session_key: 1,
                payload: serde_json::json!([{ "driver_number": 1 }]),
            },
            fetched_at: Utc::now() - ChronoDuration::seconds(2),
            attempted_at: Utc::now(),
            failure_count: 1,
            last_error: Some("OpenF1 live request failed".to_string()),
        };

        let health = live_channel_health(&[endpoint], Utc::now());

        assert_eq!(health[0].state, LiveChannelState::Cached);
        assert_eq!(health[0].rows, Some(1));
        assert!(health[0]
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("OpenF1 live request failed")));
    }

    #[test]
    fn channel_health_marks_old_failed_payload_as_stale() {
        let now = Utc::now();
        let endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "location".to_string(),
                session_key: 1,
                payload: serde_json::json!([{ "driver_number": 1 }]),
            },
            fetched_at: now - ChronoDuration::seconds(30),
            attempted_at: now,
            failure_count: 3,
            last_error: Some("OpenF1 live request failed".to_string()),
        };

        assert_eq!(
            live_channel_health(&[endpoint], now)[0].state,
            LiveChannelState::Stale
        );
    }

    #[test]
    fn failed_endpoint_retries_back_off_to_thirty_seconds() {
        assert_eq!(live_retry_delay(500, 1), ChronoDuration::seconds(1));
        assert_eq!(live_retry_delay(500, 6), ChronoDuration::seconds(16));
        assert_eq!(live_retry_delay(2_000, 6), ChronoDuration::seconds(30));
    }

    #[test]
    fn channel_health_clamps_negative_age_to_zero() {
        let now = Utc::now();
        let endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "location".to_string(),
                session_key: 1,
                payload: serde_json::json!([{ "driver_number": 1 }]),
            },
            fetched_at: now + ChronoDuration::seconds(1),
            attempted_at: now,
            failure_count: 0,
            last_error: None,
        };

        let health = live_channel_health(&[endpoint], now);

        assert_eq!(health[0].age_seconds, Some(0.0));
        assert_eq!(health[0].state, LiveChannelState::Fresh);
    }

    #[test]
    fn channel_health_uses_endpoint_cadence_for_slow_live_feeds() {
        let now = Utc::now();
        let driver_endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "drivers".to_string(),
                session_key: 1,
                payload: serde_json::json!([{ "driver_number": 1 }]),
            },
            fetched_at: now - ChronoDuration::seconds(20),
            attempted_at: now,
            failure_count: 0,
            last_error: None,
        };
        let old_weather_endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "weather".to_string(),
                session_key: 1,
                payload: serde_json::json!([{ "air_temperature": 22.0 }]),
            },
            fetched_at: now - ChronoDuration::seconds(45),
            attempted_at: now,
            failure_count: 0,
            last_error: None,
        };

        let health = live_channel_health(&[driver_endpoint, old_weather_endpoint], now);

        assert_eq!(health[0].state, LiveChannelState::Fresh);
        assert_eq!(health[1].state, LiveChannelState::Stale);
    }

    #[test]
    fn channel_health_treats_successful_empty_optional_live_feeds_as_fresh() {
        let now = Utc::now();
        let empty_pit_endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "pit".to_string(),
                session_key: 1,
                payload: serde_json::json!([]),
            },
            fetched_at: now,
            attempted_at: now,
            failure_count: 0,
            last_error: None,
        };
        let empty_driver_endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "drivers".to_string(),
                session_key: 1,
                payload: serde_json::json!([]),
            },
            fetched_at: now,
            attempted_at: now,
            failure_count: 0,
            last_error: None,
        };

        let health = live_channel_health(&[empty_pit_endpoint, empty_driver_endpoint], now);

        assert_eq!(health[0].state, LiveChannelState::Fresh);
        assert_eq!(health[0].rows, Some(0));
        assert_eq!(health[1].state, LiveChannelState::Missing);
    }

    #[test]
    fn merge_live_payload_keeps_previous_rows_and_deduplicates_new_rows() {
        let previous = RawEndpoint {
            endpoint: "position".to_string(),
            session_key: 1,
            payload: serde_json::json!([
                { "driver_number": 1, "position": 1, "date": "2026-06-28T13:00:00Z" }
            ]),
        };
        let fetched = RawEndpoint {
            endpoint: "position".to_string(),
            session_key: 1,
            payload: serde_json::json!([
                { "driver_number": 1, "position": 1, "date": "2026-06-28T13:00:00Z" },
                { "driver_number": 16, "position": 2, "date": "2026-06-28T13:00:01Z" }
            ]),
        };

        let merged = merge_live_payload(Some(previous), fetched);

        let rows = merged.payload.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["driver_number"], 1);
        assert_eq!(rows[1]["driver_number"], 16);
    }

    #[test]
    fn merge_live_payload_keeps_latest_rows_when_endpoint_exceeds_limit() {
        let previous_rows: Vec<Value> = (0..1_000)
            .map(|idx| serde_json::json!({ "driver_number": 1, "date": idx }))
            .collect();
        let fetched_rows: Vec<Value> = (1_000..1_005)
            .map(|idx| serde_json::json!({ "driver_number": 1, "date": idx }))
            .collect();
        let previous = RawEndpoint {
            endpoint: "race_control".to_string(),
            session_key: 1,
            payload: Value::Array(previous_rows),
        };
        let fetched = RawEndpoint {
            endpoint: "race_control".to_string(),
            session_key: 1,
            payload: Value::Array(fetched_rows),
        };

        let merged = merge_live_payload(Some(previous), fetched);

        let rows = merged.payload.as_array().unwrap();
        assert_eq!(rows.len(), 1_000);
        assert_eq!(rows.first().unwrap()["date"], 5);
        assert_eq!(rows.last().unwrap()["date"], 1_004);
    }

    #[test]
    fn live_incremental_since_uses_latest_row_timestamp_with_overlap() {
        let fetched_at = DateTime::parse_from_rfc3339("2026-06-28T13:00:20Z")
            .unwrap()
            .with_timezone(&Utc);
        let endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "position".to_string(),
                session_key: 1,
                payload: serde_json::json!([
                    { "driver_number": 1, "date": "2026-06-28T13:00:08Z" },
                    { "driver_number": 2, "date": "2026-06-28T13:00:10Z" }
                ]),
            },
            fetched_at,
            attempted_at: fetched_at,
            failure_count: 0,
            last_error: None,
        };

        let since = live_incremental_since(&endpoint, Some("date"));

        assert_eq!(since.to_rfc3339(), "2026-06-28T13:00:05+00:00");
    }

    #[test]
    fn live_incremental_since_falls_back_to_fetch_time_without_row_timestamp() {
        let fetched_at = DateTime::parse_from_rfc3339("2026-06-28T13:00:20Z")
            .unwrap()
            .with_timezone(&Utc);
        let endpoint = LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "position".to_string(),
                session_key: 1,
                payload: serde_json::json!([{ "driver_number": 1 }]),
            },
            fetched_at,
            attempted_at: fetched_at,
            failure_count: 0,
            last_error: None,
        };

        let since = live_incremental_since(&endpoint, Some("date"));

        assert_eq!(since.to_rfc3339(), "2026-06-28T13:00:15+00:00");
    }

    #[test]
    fn live_session_window_stays_open_during_post_session_padding() {
        let session = test_session("2026-06-28T13:00:00Z", "2026-06-28T15:00:00Z");
        let now = DateTime::parse_from_rfc3339("2026-06-28T15:29:59Z")
            .unwrap()
            .with_timezone(&Utc);

        assert!(!live_session_window_closed(&session, now));
    }

    #[test]
    fn live_snapshot_time_clamps_during_post_session_padding() {
        let session = test_session("2026-06-28T13:00:00Z", "2026-06-28T15:00:00Z");
        let now = DateTime::parse_from_rfc3339("2026-06-28T15:20:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let (snapshot_t, max_t) = live_snapshot_time_bounds(&session, now);

        assert_eq!(max_t, 8_400.0);
        assert_eq!(snapshot_t, 8_400.0);
    }

    #[test]
    fn live_snapshot_time_uses_default_duration_when_end_time_is_missing() {
        let session = test_session("2026-06-28T13:00:00Z", "");
        let now = DateTime::parse_from_rfc3339("2026-06-28T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let (snapshot_t, max_t) = live_snapshot_time_bounds(&session, now);

        assert_eq!(snapshot_t, 3_600.0);
        assert_eq!(max_t, 10_800.0);
    }

    #[test]
    fn live_snapshot_time_uses_default_duration_when_end_time_is_before_start() {
        let session = test_session("2026-06-28T13:00:00Z", "2026-06-28T12:00:00Z");
        let now = DateTime::parse_from_rfc3339("2026-06-28T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let (snapshot_t, max_t) = live_snapshot_time_bounds(&session, now);

        assert_eq!(snapshot_t, 3_600.0);
        assert_eq!(max_t, 10_800.0);
        assert!(!live_session_window_closed(&session, now));
    }

    #[test]
    fn live_session_window_closes_after_padding() {
        let session = test_session("2026-06-28T13:00:00Z", "2026-06-28T15:00:00Z");
        let now = DateTime::parse_from_rfc3339("2026-06-28T15:30:01Z")
            .unwrap()
            .with_timezone(&Utc);

        assert!(live_session_window_closed(&session, now));
    }

    fn test_session(start_time: &str, end_time: &str) -> Session {
        Session {
            session_key: 1,
            meeting_key: 2,
            year: 2026,
            name: "Race".to_string(),
            session_type: SessionType::Race,
            start_time: start_time.to_string(),
            end_time: end_time.to_string(),
            total_laps: 0,
        }
    }
}
