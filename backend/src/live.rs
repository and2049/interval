use crate::{
    connectors::{
        openf1_historical::RawEndpoint,
        openf1_live::{
            live_endpoint_cadence_seconds, OpenF1Auth, OpenF1LiveClient, OpenF1LiveError,
        },
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
// OpenF1's live feed lags real time by a few seconds (measured ~2.6 s at Monza 2026)
// and we only poll location every few seconds, so the newest sample on hand can be
// up to (upstream lag + location cadence + request time) old. The cursor runs behind
// wall clock by that much plus a margin, so it always has real samples on both sides
// to interpolate between. Too short a delay makes the cursor catch up with the data
// horizon, freeze there until the next poll lands, then jump: the cars stutter.
const LIVE_UPSTREAM_LAG_ALLOWANCE_SECONDS: f64 = 8.0;

/// Location poll cadence plus the upstream allowance. Derived, not fixed, so a change
/// to the polling budget cannot silently reintroduce the stutter.
fn live_display_delay_seconds() -> f64 {
    live_endpoint_cadence_seconds("location") + LIVE_UPSTREAM_LAG_ALLOWANCE_SECONDS
}
const LIVE_WINDOW_PADDING_MINUTES: i64 = 30;
const LIVE_QUIET_SHUTDOWN_MINUTES: i64 = 10;
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

    /// Applies new OpenF1 credentials to the shared client.
    ///
    /// A running live session is deliberately left alone: the client's config is shared,
    /// so the session's next poll uses the new login on its own. Tearing the session down
    /// would take the user's race off the screen for no benefit.
    pub async fn apply_openf1_auth(&self, auth: OpenF1Auth) {
        self.client.set_auth(auth).await;
    }

    pub async fn probe_openf1(&self) -> Result<(), OpenF1LiveError> {
        self.client.probe().await
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
        if live_session_window_closed(&existing.session, &existing.raw_bundle, Utc::now()) {
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
        if sessions.get(&session_key).is_some_and(|session| {
            live_session_window_closed(&session.session, &session.raw_bundle, Utc::now())
        }) {
            sessions.remove(&session_key);
            return None;
        }
        sessions.get(&session_key).cloned()
    }

    async fn current_active_session(&self) -> Option<OpenF1LiveSession> {
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, session| {
            !live_session_window_closed(&session.session, &session.raw_bundle, Utc::now())
        });
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
        let latest_location_t = data
            .locations
            .iter()
            .map(|location| location.t)
            .max_by(f64::total_cmp);
        let (snapshot_t, max_t) = live_snapshot_time_bounds(&session, Utc::now(), latest_location_t);
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
            let specs = self.client.endpoint_specs();
            let endpoint_fetches = specs.iter().map(|spec| {
                let client = self.client.clone();
                async move {
                    let fetched = client
                        .fetch_initial_live_bundle_endpoint(session_key, spec)
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
        let specs = self.client.endpoint_specs();
        let endpoint_fetches = specs.iter().map(|spec| {
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
                            .fetch_initial_live_bundle_endpoint(session_key, spec)
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
        // A malformed (non-array) payload must not clobber accumulated rows.
        fetched.payload = Value::Array(rows);
        return fetched;
    };

    let mut indexes = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| live_row_key(&fetched.endpoint, row).map(|key| (key, index)))
        .collect::<HashMap<_, _>>();
    for row in fetched_rows {
        let Some(key) = live_row_key(&fetched.endpoint, &row) else {
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

fn live_row_key(endpoint: &str, row: &Value) -> Option<String> {
    let driver = row.get("driver_number").and_then(Value::as_i64);
    let key = match endpoint {
        // Rows on these endpoints mutate as data fills in (sector times,
        // stint lap_end, ...), so key on row identity to replace older
        // versions instead of accumulating duplicates.
        "laps" => driver
            .zip(row.get("lap_number").and_then(Value::as_i64))
            .map(|(driver, lap)| format!("{driver}:{lap}")),
        "stints" => driver
            .zip(row.get("stint_number").and_then(Value::as_i64))
            .map(|(driver, stint)| format!("{driver}:{stint}")),
        "drivers" | "session_result" => driver.map(|driver| driver.to_string()),
        // Distinct race control messages routinely share a timestamp, so the
        // full row is the only collision-free key.
        "race_control" => None,
        _ => row
            .get("date")
            .and_then(Value::as_str)
            .map(|date| format!("{}:{date}", driver.unwrap_or_default())),
    };
    key.or_else(|| serde_json::to_string(row).ok())
}

fn live_session_with_refresh_error(
    mut session: OpenF1LiveSession,
    error: anyhow::Error,
) -> OpenF1LiveSession {
    let now = Utc::now();
    let message = format!("OpenF1 live refresh failed: {error}");
    if let Some(entry) = session
        .raw_bundle
        .iter_mut()
        .find(|entry| entry.raw.endpoint == "refresh")
    {
        entry.fetched_at = now;
        entry.attempted_at = now;
        entry.failure_count = entry.failure_count.saturating_add(1);
        entry.last_error = Some(message);
    } else {
        session.raw_bundle.push(LiveCachedEndpoint {
            raw: RawEndpoint {
                endpoint: "refresh".to_string(),
                session_key: session.session.session_key,
                payload: Value::Array(vec![]),
            },
            fetched_at: now,
            attempted_at: now,
            failure_count: 1,
            last_error: Some(message),
        });
    }
    session.updated_at = now.to_rfc3339();
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
            storage::replace_track_geometry(pool, &geometry).await?;
            return Ok(geometry);
        }
    }
    if let Some(geometry) = replay::track_geometry_builder::build_track_geometry_from_lap(
        session.session_key,
        data,
        TrackGeometrySource::OpenF1Location,
    ) {
        // Persist so the map stays stable across refreshes instead of being
        // rebuilt from a sliding telemetry window, and so later sessions at
        // this meeting can reuse it.
        storage::replace_track_geometry(pool, &geometry).await?;
        return Ok(geometry);
    }

    // No clean flying lap in the window yet: use the curated/schematic
    // fallback rather than a degenerate shape built from partial traces.
    Ok(replay::track_geometry_builder::build_track_geometry(
        session.session_key,
        &[],
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

fn live_snapshot_time_bounds(
    session: &Session,
    now: DateTime<Utc>,
    latest_location_t: Option<f64>,
) -> (f64, f64) {
    let now_t = t_since_session_start(session, now).max(0.0);
    let max_t = session_duration(session)
        .unwrap_or(now_t + LIVE_FRAME_STEP_SECONDS)
        .max(now_t);
    let target_t = now_t - live_display_delay_seconds();
    let snapshot_t = latest_location_t
        .map_or(target_t, |data_t| data_t.min(target_t))
        .max(0.0)
        .min(max_t);
    (snapshot_t, max_t)
}

fn live_session_window_closed(
    session: &Session,
    bundle: &[LiveCachedEndpoint],
    now: DateTime<Utc>,
) -> bool {
    let Ok(start) = DateTime::parse_from_rfc3339(&session.start_time) else {
        return false;
    };
    let start = start.with_timezone(&Utc);
    let end = live_session_end_or_default(start, &session.end_time);
    if now <= end + ChronoDuration::minutes(LIVE_WINDOW_PADDING_MINUTES) {
        return false;
    }
    // Red flags routinely push races long past their scheduled end, so past
    // the padded window only close once the feed has gone quiet.
    match latest_live_row_time(bundle) {
        Some(latest) => {
            now.signed_duration_since(latest)
                > ChronoDuration::minutes(LIVE_QUIET_SHUTDOWN_MINUTES)
        }
        None => true,
    }
}

fn latest_live_row_time(bundle: &[LiveCachedEndpoint]) -> Option<DateTime<Utc>> {
    bundle
        .iter()
        .filter_map(|endpoint| latest_row_timestamp(&endpoint.raw.payload, "date"))
        .max()
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

