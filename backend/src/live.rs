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

