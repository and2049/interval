//! The async half of the replay store: the port of `frontend/src/stores/replay.ts`'s
//! effects, EventSources, timers and request orchestration onto tokio.
//!
//! Structure: every mutation goes through [`Shared::update`], which bumps a watch
//! channel; one reconciler task re-runs the frontend's `createEffect` bodies after
//! every change (they are idempotent, so this converges exactly like Solid's dependency
//! tracking) and reconciles the three SSE stream tasks against the desired state,
//! comparing by value the way Solid's equality-deduped signals did. Superseded async
//! work is fenced two ways, both ports of the original: generation counters inside the
//! store (`*_request_id`) and a per-stream gate (`Arc<AtomicU64>`) standing in for the
//! `if (stream !== source) return` identity checks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use interval_backend::domain::{LiveAvailability, ReplayEvent, ReplayMetadata, ReplaySnapshot};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::{LiveConnection, ReplayStore, SimConnection};
use crate::api_client::{ApiClient, ApiError};
use crate::playback;
use crate::sse::SseParser;

const PLAYBACK_TICK: Duration = Duration::from_millis(100);
const LIVE_AVAILABILITY_POLL_TICK: Duration = Duration::from_millis(1000);
const LIVE_STATUS_REFRESH_MIN_INTERVAL: Duration = Duration::from_millis(2000);

/// Why a live-availability check runs; only non-poll checks show the transient
/// "Checking live race status..." message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckSource {
    Startup,
    Manual,
    Poll,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ReplayStreamConfig {
    session_key: i64,
    from: f64,
    speed: f64,
}

struct StreamTask<C> {
    config: C,
    handle: JoinHandle<()>,
}

#[derive(Default)]
struct Aux {
    replay_stream: Option<StreamTask<ReplayStreamConfig>>,
    sim_stream: Option<StreamTask<i64>>,
    live_stream: Option<StreamTask<(i64, u64)>>,
    resource_fetch: Option<(i64, u64)>,
    resource_task: Option<JoinHandle<()>>,
    paused_snapshot: Option<(i64, u64, u64)>,
    reconnect_timer: Option<JoinHandle<()>>,
    last_status_refresh: Option<Instant>,
    last_availability_check: Option<Instant>,
    last_persisted_session_key: Option<Option<i64>>,
}

struct Shared {
    state: Mutex<ReplayStore>,
    aux: Mutex<Aux>,
    api: ApiClient,
    tokio: tokio::runtime::Handle,
    changed: watch::Sender<u64>,
    repaint: mpsc::UnboundedSender<()>,
    persist_session_key: Box<dyn Fn(Option<i64>) + Send + Sync>,
    // The `stream !== source` identity checks: each spawn takes the next generation
    // and every state application from a stream task verifies its generation is still
    // the current one for its kind.
    replay_gate: AtomicU64,
    sim_gate: AtomicU64,
    live_gate: AtomicU64,
}

impl Shared {
    /// Locks the store, applies `f`, then wakes the reconciler and the UI.
    fn update<R>(&self, f: impl FnOnce(&mut ReplayStore) -> R) -> R {
        let result = {
            let mut state = self.state.lock().expect("store lock");
            f(&mut state)
        };
        self.changed.send_modify(|version| *version += 1);
        let _ = self.repaint.send(());
        result
    }

    fn read<R>(&self, f: impl FnOnce(&ReplayStore) -> R) -> R {
        f(&self.state.lock().expect("store lock"))
    }
}

pub struct StoreRuntime {
    shared: Arc<Shared>,
}

impl StoreRuntime {
    /// Spawns the reconciler, the playback ticker, the availability poll, and the
    /// startup live check. Must be called from within a tokio runtime context.
    /// `repaint` receives one message per state change; `persist_session_key` is
    /// called with the key to remember (`None` clears it).
    pub fn start(
        api: ApiClient,
        stored_session_key: Option<i64>,
        persist_session_key: Box<dyn Fn(Option<i64>) + Send + Sync>,
        repaint: mpsc::UnboundedSender<()>,
    ) -> Arc<Self> {
        let (changed, _) = watch::channel(0u64);
        let shared = Arc::new(Shared {
            state: Mutex::new(ReplayStore::new(stored_session_key)),
            aux: Mutex::new(Aux {
                last_persisted_session_key: Some(stored_session_key),
                ..Aux::default()
            }),
            api,
            tokio: tokio::runtime::Handle::current(),
            changed,
            repaint,
            persist_session_key,
        replay_gate: AtomicU64::new(0),
            sim_gate: AtomicU64::new(0),
            live_gate: AtomicU64::new(0),
        });

        shared
            .tokio
            .spawn(reconciler_task(Arc::clone(&shared)));
        shared
            .tokio
            .spawn(playback_tick_task(Arc::clone(&shared)));
        shared
            .tokio
            .spawn(availability_poll_task(Arc::clone(&shared)));
        // The checkedLiveOnStartup effect.
        shared.tokio.spawn({
            let shared = Arc::clone(&shared);
            async move { check_openf1_live(&shared, CheckSource::Startup).await }
        });

        Arc::new(Self { shared })
    }

    /// Read access for rendering. Never hold across an await.
    pub fn state(&self) -> MutexGuard<'_, ReplayStore> {
        self.shared.state.lock().expect("store lock")
    }

    /// A change-notification subscription: the receiver resolves after every store
    /// mutation. Used by the selector runtime to mirror the active session.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.shared.changed.subscribe()
    }

    pub fn set_playing(&self, playing: bool) {
        self.shared.update(|store| store.set_playing(playing));
    }

    pub fn toggle_playing(&self) {
        self.shared.update(|store| store.toggle_playing());
    }

    pub fn set_speed(&self, speed: f64) {
        self.shared.update(|store| store.set_speed(speed));
    }

    pub fn seek(&self, time: f64) {
        let target = self.shared.update(|store| store.seek_time(time));
        if let Some(target) = target {
            let shared = Arc::clone(&self.shared);
            self.shared.tokio.spawn(async move {
                load_snapshot(&shared, Some(target)).await;
            });
        }
    }

    pub fn check_live(&self) {
        let shared = Arc::clone(&self.shared);
        self.shared.tokio.spawn(async move {
            check_openf1_live(&shared, CheckSource::Manual).await;
        });
    }

    pub fn stop_live(&self) {
        let shared = Arc::clone(&self.shared);
        self.shared.tokio.spawn(async move {
            stop_live(&shared).await;
        });
    }

    pub fn toggle_live_simulation(&self) {
        let shared = Arc::clone(&self.shared);
        self.shared.tokio.spawn(async move {
            toggle_live_simulation(&shared).await;
        });
    }

    pub fn open_session(&self, key: i64) {
        stop_abandoned_live_runtimes(&self.shared);
        self.shared.update(|store| {
            store.reset_for_session_change();
            if playback::should_reload_session(store.session_key, key) {
                store.resources_refetch_nonce += 1;
            } else {
                store.session_key = Some(key);
            }
        });
    }

    pub fn clear_active_session(&self, next_intent_key: Option<i64>) {
        let is_noop = self
            .shared
            .read(|store| next_intent_key.is_some() && next_intent_key == store.session_key);
        if is_noop {
            return;
        }
        stop_abandoned_live_runtimes(&self.shared);
        self.shared.update(|store| {
            store.reset_for_session_change();
            store.session_key = None;
        });
    }

    /// Starts a live session for an explicitly chosen key (the OPEN LIVE button).
    pub fn open_live(&self, key: i64) {
        let shared = Arc::clone(&self.shared);
        self.shared.tokio.spawn(async move {
            start_openf1_live(&shared, key).await;
        });
    }
}

// ---- The effect runner ----

async fn reconciler_task(shared: Arc<Shared>) {
    let mut rx = shared.changed.subscribe();
    loop {
        rx.borrow_and_update();
        reconcile(&shared);
        if rx.changed().await.is_err() {
            return;
        }
    }
}

fn reconcile(shared: &Arc<Shared>) {
    run_clear_missing_replay_effect(shared);
    run_persist_session_key_effect(shared);
    run_session_init_effect(shared);
    reconcile_resources(shared);
    reconcile_paused_snapshot(shared);
    reconcile_replay_stream(shared);
    reconcile_sim_stream(shared);
    reconcile_live_stream(shared);
}

/// A metadata 404 for a stored historical session means the cache is gone: forget the
/// stored key so the app falls back to the selector instead of an error screen.
fn run_clear_missing_replay_effect(shared: &Arc<Shared>) {
    let should_clear = shared.read(|store| {
        playback::should_clear_missing_historical_replay(
            playback::ClearMissingHistoricalReplayOptions {
                metadata_error: store.metadata.error.as_deref(),
                metadata_loading: store.metadata.loading,
                session_key: store.session_key,
                live_active: store.live_active,
                live_simulation_active: store.live_simulation_active,
            },
        )
    });
    if should_clear {
        (shared.persist_session_key)(None);
        shared.aux.lock().expect("aux lock").last_persisted_session_key = Some(None);
        shared.update(|store| store.session_key = None);
    }
}

fn run_persist_session_key_effect(shared: &Arc<Shared>) {
    let key = shared.read(|store| store.active_metadata().map(|meta| meta.session.session_key));
    let Some(key) = key else { return };
    let mut aux = shared.aux.lock().expect("aux lock");
    if aux.last_persisted_session_key != Some(Some(key)) {
        aux.last_persisted_session_key = Some(Some(key));
        drop(aux);
        (shared.persist_session_key)(Some(key));
    }
}

/// When a session's metadata first becomes active, seek to the race start and load the
/// opening frame.
fn run_session_init_effect(shared: &Arc<Shared>) {
    // Read-only pre-check: `update` always wakes the reconciler, so calling it from
    // an effect that usually changes nothing would spin the reconcile loop forever
    // (and the repaint flood starves the UI thread's platform event loop).
    let needs_init = shared.read(|store| {
        store.active_metadata().is_some_and(|meta| {
            !store.playing
                && !store.live_simulation_active
                && !store.live_active
                && !store.live_transitioning()
                && store.initialized_session_key != Some(meta.session.session_key)
        })
    });
    if !needs_init {
        return;
    }
    let start = shared.update(|store| {
        let Some(meta) = store.active_metadata() else {
            return None;
        };
        if store.playing
            || store.live_simulation_active
            || store.live_active
            || store.live_transitioning()
        {
            return None;
        }
        let key = meta.session.session_key;
        if store.initialized_session_key == Some(key) {
            return None;
        }
        let start_t = if meta.race_start_t > 0.0 {
            meta.race_start_t
        } else {
            meta.min_t
        };
        store.initialized_session_key = Some(key);
        store.time = start_t;
        store.stream_start_time = start_t;
        Some(start_t)
    });
    if let Some(start_t) = start {
        let shared = Arc::clone(shared);
        shared.tokio.clone().spawn(async move {
            load_snapshot(&shared, Some(start_t)).await;
        });
    }
}

// ---- Resource fetches (metadata → geometry + events) ----

fn reconcile_resources(shared: &Arc<Shared>) {
    let desired = shared.read(|store| {
        store
            .session_key
            .map(|key| (key, store.resources_refetch_nonce))
    });
    let mut aux = shared.aux.lock().expect("aux lock");
    if aux.resource_fetch == desired {
        return;
    }
    if let Some(task) = aux.resource_task.take() {
        task.abort();
    }
    aux.resource_fetch = desired;
    let Some((key, _nonce)) = desired else { return };
    let generation = desired;
    let shared_task = Arc::clone(shared);
    aux.resource_task = Some(shared.tokio.spawn(async move {
        fetch_session_resources(&shared_task, key, generation).await;
    }));
}

async fn fetch_session_resources(shared: &Arc<Shared>, key: i64, generation: Option<(i64, u64)>) {
    let is_current =
        |shared: &Shared| shared.aux.lock().expect("aux lock").resource_fetch == generation;

    shared.update(|store| {
        store.metadata.loading = true;
        store.metadata.error = None;
    });
    let metadata = shared.api.metadata(key).await;
    if !is_current(shared) {
        return;
    }
    let proceed = shared.update(|store| {
        store.metadata.loading = false;
        store.metadata.key = Some(key);
        match metadata {
            Ok(value) => {
                store.metadata.value = Some(value);
                store.metadata.error = None;
                // The dependent resources only fire while the metadata actually
                // belongs to the selected session (replayResourceSessionKey).
                store.resource_session_key() == Some(key)
            }
            Err(error) => {
                store.metadata.error = Some(error.to_string());
                false
            }
        }
    });
    if !proceed {
        return;
    }

    shared.update(|store| {
        store.track_geometry.loading = true;
        store.track_geometry.error = None;
        store.events.loading = true;
        store.events.error = None;
    });
    let (geometry, events) = tokio::join!(
        shared.api.track_geometry(key),
        shared.api.events(key)
    );
    if !is_current(shared) {
        return;
    }
    shared.update(|store| {
        store.track_geometry.loading = false;
        store.track_geometry.key = Some(key);
        match geometry {
            Ok(value) => {
                store.track_geometry.value = Some(value);
                store.track_geometry.error = None;
            }
            Err(error) => store.track_geometry.error = Some(error.to_string()),
        }
        store.events.loading = false;
        store.events.key = Some(key);
        match events {
            Ok(value) => {
                store.events.value = Some(value.events);
                store.events.error = None;
            }
            Err(error) => store.events.error = Some(error.to_string()),
        }
    });
}

// ---- Paused-frame snapshot loading ----

/// The `loadSnapshot(time())` effect: while paused on a historical session, keep the
/// displayed frame in sync with the cursor. Deduped on the quantized frame, which is
/// what re-running the Solid effect converged to as well.
fn reconcile_paused_snapshot(shared: &Arc<Shared>) {
    let desired = shared.read(|store| {
        let meta = store.active_metadata()?;
        if store.playing
            || store.live_simulation_active
            || store.live_active
            || store.live_transitioning()
        {
            return None;
        }
        let request = playback::snapshot_request(store.session_key, Some(meta), store.time)?;
        Some((request.key, request.t.to_bits(), store.resources_refetch_nonce))
    });
    let Some(desired) = desired else { return };
    {
        let mut aux = shared.aux.lock().expect("aux lock");
        if aux.paused_snapshot == Some(desired) {
            return;
        }
        aux.paused_snapshot = Some(desired);
    }
    let shared = Arc::clone(shared);
    shared.tokio.clone().spawn(async move {
        load_snapshot(&shared, Some(f64::from_bits(desired.1))).await;
    });
}

async fn load_snapshot(shared: &Arc<Shared>, t: Option<f64>) {
    let request = shared.update(|store| {
        let t = t.unwrap_or(store.time);
        let request =
            playback::snapshot_request(store.session_key, store.metadata.value.as_ref(), t)?;
        store.snapshot_request_id += 1;
        store.snapshot_loading = true;
        store.snapshot_error = None;
        Some((request, store.snapshot_request_id))
    });
    let Some((request, request_id)) = request else {
        return;
    };
    let result = shared.api.snapshot(request.key, request.t).await;
    shared.update(|store| {
        let applies = playback::should_apply_snapshot_result(
            request_id,
            store.snapshot_request_id,
            request.key,
            store.session_key,
            store.live_active,
            store.live_simulation_active,
            store.live_transitioning(),
        );
        if !applies {
            return;
        }
        match result {
            Ok(snapshot) => {
                if Some(snapshot.cursor.session_key) == store.session_key {
                    store.current_snapshot = Some(snapshot);
                }
            }
            Err(error) => store.snapshot_error = Some(error.to_string()),
        }
        store.snapshot_loading = false;
    });
}

// ---- Timers ----

async fn playback_tick_task(shared: Arc<Shared>) {
    let mut last_tick = Instant::now();
    let mut interval = tokio::time::interval(PLAYBACK_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let now = Instant::now();
        let elapsed = now.duration_since(last_tick).as_secs_f64();
        last_tick = now;
        // Only notify when something moved, so an idle app stays idle.
        let changed = {
            let mut state = shared.state.lock().expect("store lock");
            state.apply_tick(elapsed)
        };
        if changed {
            shared.changed.send_modify(|version| *version += 1);
            let _ = shared.repaint.send(());
        }
    }
}

async fn availability_poll_task(shared: Arc<Shared>) {
    let mut interval = tokio::time::interval(LIVE_AVAILABILITY_POLL_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let due = {
            let aux = shared.aux.lock().expect("aux lock");
            let poll_delay_ms = shared.read(|store| {
                playback::live_availability_poll_delay_ms(
                    &store.live_availability,
                    store.live_availability_message.as_deref(),
                )
            });
            match aux.last_availability_check {
                Some(last) => {
                    poll_delay_ms.is_finite()
                        && last.elapsed().as_millis() as f64 >= poll_delay_ms
                }
                None => true,
            }
        };
        let should_check = due
            && shared.read(|store| {
                playback::should_poll_live_availability(&store.live_availability)
                    && !store.live_availability_checking
                    && !store.live_active
                    && !store.live_simulation_active
            });
        if should_check {
            check_openf1_live(&shared, CheckSource::Poll).await;
        }
    }
}

// ---- Live availability / live start orchestration ----

async fn check_openf1_live(shared: &Arc<Shared>, source: CheckSource) {
    let request_id = shared.update(|store| {
        store.openf1_live_check_request_id += 1;
        store.openf1_live_start_request_id += 1;
        store.live_availability_checking = true;
        if source != CheckSource::Poll {
            store.live_availability_message = Some("Checking live race status...".to_string());
        }
        store.openf1_live_check_request_id
    });
    shared.aux.lock().expect("aux lock").last_availability_check = Some(Instant::now());

    let current = shared.api.live_current().await;
    let is_current =
        |store: &ReplayStore| playback::should_apply_live_start_result(request_id, store.openf1_live_check_request_id);

    let start_key = shared.update(|store| {
        if !is_current(store) {
            return None;
        }
        match &current {
            Ok(current) => {
                store.live_availability = current.availability.clone();
                let key = current.session.as_ref().map(|session| session.session_key);
                match key {
                    Some(key) if current.active => {
                        store.live_availability_message =
                            Some("Opening active OpenF1 live session...".to_string());
                        Some(key)
                    }
                    _ => {
                        store.live_availability_message = playback::live_current_message(
                            &current.availability,
                            current.message.as_deref(),
                            current.next_session.as_ref(),
                            current.next_meeting.as_ref(),
                        );
                        None
                    }
                }
            }
            Err(error) => {
                store.live_availability = LiveAvailability::Error;
                store.live_availability_message =
                    Some(playback::live_check_error_message(Some(&error.to_string())));
                None
            }
        }
    });

    if let Some(key) = start_key {
        if start_openf1_live(shared, key).await {
            shared.update(|store| {
                if is_current(store) {
                    store.live_availability_message = None;
                }
            });
        }
    }

    shared.update(|store| {
        if is_current(store) {
            store.live_availability_checking = false;
        }
    });
}

async fn start_openf1_live(shared: &Arc<Shared>, key: i64) -> bool {
    let (request_id, simulation_stop_key) = shared.update(|store| {
        store.openf1_live_start_request_id += 1;
        store.live_simulation_start_request_id += 1;
        store.playing = false;
        let simulation_stop_key = store.live_simulation_active.then_some(store.session_key).flatten();
        (store.openf1_live_start_request_id, simulation_stop_key)
    });

    let mut runtime_started = false;
    macro_rules! stop_if_superseded {
        () => {{
            let superseded = shared.read(|store| {
                !playback::should_apply_live_start_result(
                    request_id,
                    store.openf1_live_start_request_id,
                )
            });
            if superseded {
                if runtime_started {
                    let _ = shared.api.live_stop(key).await;
                }
                return false;
            }
        }};
    }

    if let Some(simulation_key) = simulation_stop_key {
        let _ = shared.api.live_simulation_stop(simulation_key).await;
    }
    stop_if_superseded!();

    clear_openf1_live_reconnect(shared);
    let stored_fallback = shared.read(|store| store.return_session_key_after_live);
    shared.update(|store| {
        store.live_simulation_active = false;
        store.live_simulation_connection = SimConnection::Idle;
        store.live_events.clear();
        store.snapshot_error = None;
        store.snapshot_loading = true;
        store.live_connection = LiveConnection::Connecting;
        store.return_session_key_after_live = stored_fallback.or(store.session_key);
        store.snapshot_request_id += 1;
        store.current_snapshot = None;
    });

    let started = shared.api.live_start(key).await;
    match started {
        Ok(_) => {
            runtime_started = true;
        }
        Err(error) => {
            stop_if_superseded!();
            apply_live_start_error(shared, &error);
            return false;
        }
    }
    stop_if_superseded!();
    let status = shared.api.live_status(key).await.ok();
    stop_if_superseded!();
    let metadata = match shared.api.live_metadata(key).await {
        Ok(metadata) => metadata,
        Err(error) => {
            stop_if_superseded!();
            let _ = shared.api.live_stop(key).await;
            apply_live_start_error(shared, &error);
            return false;
        }
    };
    stop_if_superseded!();
    let snapshot = match shared.api.live_snapshot(key).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            stop_if_superseded!();
            let _ = shared.api.live_stop(key).await;
            apply_live_start_error(shared, &error);
            return false;
        }
    };
    stop_if_superseded!();
    let geometry = shared.api.live_track_geometry(key).await.ok();
    stop_if_superseded!();
    let events = shared.api.live_events(key).await.ok();
    stop_if_superseded!();

    shared.update(|store| {
        store.session_key = Some(key);
        if let Some(status) = status.filter(|status| status.session_key == key) {
            store.live_status = Some(status);
        }
        if metadata.session.session_key == key {
            store.live_metadata = Some(metadata);
        }
        if snapshot.cursor.session_key == key {
            store.time = snapshot.cursor.t;
            store.current_snapshot = Some(snapshot);
        }
        if let Some(geometry) = geometry.filter(|geometry| geometry.session_key == key) {
            store.live_geometry = Some(geometry);
        }
        if let Some(events) = events {
            store.live_events = events.events;
        }
        store.live_active = true;
        store.live_connection = LiveConnection::Connected;
        store.snapshot_loading = false;
    });
    true
}

fn apply_live_start_error(shared: &Arc<Shared>, error: &ApiError) {
    let text = error.to_string();
    shared.update(|store| {
        store.snapshot_error = Some(text.clone());
        store.live_availability = playback::live_availability_after_start_error(Some(&text));
        store.live_availability_message = Some(playback::live_start_error_message(Some(&text)));
        store.live_connection = LiveConnection::Disconnected;
        store.live_active = false;
        store.clear_openf1_live_resources();
        store.return_session_key_after_live = None;
        store.snapshot_loading = false;
    });
}

async fn stop_live(shared: &Arc<Shared>) {
    let key = shared.update(|store| {
        store.openf1_live_check_request_id += 1;
        store.openf1_live_start_request_id += 1;
        store.live_availability_checking = false;
        store.session_key
    });
    clear_openf1_live_reconnect(shared);
    if let Some(key) = key {
        let _ = shared.api.live_stop(key).await;
    }
    shared.update(|store| {
        let return_key =
            playback::session_key_after_live_stops(store.session_key, store.return_session_key_after_live);
        store.live_active = false;
        store.live_availability = LiveAvailability::Inactive;
        store.live_connection = LiveConnection::Idle;
        store.clear_live_runtime_resources();
        store.session_key = return_key;
        store.return_session_key_after_live = None;
        store.live_availability_message = Some("Live session stopped.".to_string());
    });
}

fn stop_abandoned_live_runtimes(shared: &Arc<Shared>) {
    let (live_key, simulation_key) = shared.read(|store| {
        (
            playback::openf1_live_session_key_to_stop(store.session_key, store.live_active),
            playback::live_simulation_session_key_to_stop(
                store.session_key,
                store.live_simulation_active,
            ),
        )
    });
    if let Some(key) = live_key {
        let api = shared.api.clone();
        shared.tokio.spawn(async move {
            let _ = api.live_stop(key).await;
        });
    }
    if let Some(key) = simulation_key {
        let api = shared.api.clone();
        shared.tokio.spawn(async move {
            let _ = api.live_simulation_stop(key).await;
        });
    }
}

async fn toggle_live_simulation(shared: &Arc<Shared>) {
    let (request_id, key) = shared.update(|store| {
        store.live_simulation_start_request_id += 1;
        store.openf1_live_check_request_id += 1;
        store.openf1_live_start_request_id += 1;
        store.live_availability_checking = false;
        (store.live_simulation_start_request_id, store.session_key)
    });
    let Some(key) = key else { return };

    let is_current = |store: &ReplayStore| {
        playback::should_apply_live_start_result(request_id, store.live_simulation_start_request_id)
            && store.session_key == Some(key)
    };
    let mut runtime_started = false;
    macro_rules! stop_if_superseded {
        () => {{
            if !shared.read(|store| is_current(store)) {
                if runtime_started {
                    let _ = shared.api.live_simulation_stop(key).await;
                }
                return;
            }
        }};
    }

    let (active, connecting) = shared.read(|store| {
        (
            store.live_simulation_active,
            store.live_simulation_connection == SimConnection::Connecting,
        )
    });

    // A connect still in flight: cancel it.
    if !active && connecting {
        let _ = shared.api.live_simulation_stop(key).await;
        stop_if_superseded!();
        shared.update(|store| {
            store.live_simulation_connection = SimConnection::Idle;
            store.snapshot_loading = false;
            store.current_snapshot = None;
        });
        return;
    }

    if active {
        let _ = shared.api.live_simulation_stop(key).await;
        stop_if_superseded!();
        shared.update(|store| {
            store.live_simulation_active = false;
            store.live_simulation_connection = SimConnection::Idle;
            store.clear_live_runtime_resources();
        });
        return;
    }

    shared.update(|store| store.playing = false);
    stop_abandoned_live_runtimes(shared);
    clear_openf1_live_reconnect(shared);
    shared.update(|store| {
        store.live_active = false;
        store.live_connection = LiveConnection::Idle;
        store.live_status = None;
        store.live_events.clear();
        store.snapshot_error = None;
        store.snapshot_loading = true;
        store.live_simulation_connection = SimConnection::Connecting;
        store.snapshot_request_id += 1;
        store.current_snapshot = None;
    });

    match shared.api.live_simulation_start(key).await {
        Ok(_) => runtime_started = true,
        Err(error) => {
            stop_if_superseded!();
            let text = error.to_string();
            shared.update(|store| {
                store.snapshot_error = Some(text);
                store.live_simulation_connection = SimConnection::Disconnected;
                store.live_simulation_active = false;
                store.snapshot_loading = false;
            });
            return;
        }
    }
    stop_if_superseded!();
    let snapshot = match shared.api.live_simulation_snapshot(key).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            stop_if_superseded!();
            let text = error.to_string();
            shared.update(|store| {
                if is_current(store) {
                    store.snapshot_error = Some(text);
                    store.live_simulation_connection = SimConnection::Disconnected;
                    store.live_simulation_active = false;
                    store.snapshot_loading = false;
                }
            });
            return;
        }
    };
    stop_if_superseded!();
    let mismatched = shared.read(|store| Some(snapshot.cursor.session_key) != store.session_key);
    if mismatched {
        let _ = shared.api.live_simulation_stop(key).await;
        return;
    }
    shared.update(|store| {
        store.time = snapshot.cursor.t;
        store.current_snapshot = Some(snapshot);
        store.live_simulation_active = true;
        if is_current(store) {
            store.snapshot_loading = false;
        }
    });
}

// ---- SSE stream reconciliation ----

fn next_generation(gate: &AtomicU64) -> u64 {
    gate.fetch_add(1, Ordering::SeqCst) + 1
}

fn gate_current(gate: &AtomicU64, generation: u64) -> bool {
    gate.load(Ordering::SeqCst) == generation
}

fn reconcile_replay_stream(shared: &Arc<Shared>) {
    let desired = shared.read(|store| {
        let meta = store.active_metadata()?;
        (store.playing && !store.live_simulation_active && !store.live_active).then(|| {
            ReplayStreamConfig {
                session_key: meta.session.session_key,
                from: store.stream_start_time,
                speed: store.speed,
            }
        })
    });
    let mut aux = shared.aux.lock().expect("aux lock");
    if aux.replay_stream.as_ref().map(|task| task.config) == desired {
        return;
    }
    if let Some(task) = aux.replay_stream.take() {
        task.handle.abort();
    }
    let generation = next_generation(&shared.replay_gate);
    let Some(config) = desired else { return };
    shared.update(|store| {
        store.snapshot_loading = true;
        store.snapshot_error = None;
    });
    let shared_task = Arc::clone(shared);
    let handle = shared.tokio.spawn(async move {
        run_replay_stream(shared_task, config, generation).await;
    });
    aux.replay_stream = Some(StreamTask { config, handle });
}

async fn run_replay_stream(shared: Arc<Shared>, config: ReplayStreamConfig, generation: u64) {
    let url = shared
        .api
        .stream_url(config.session_key, config.from, config.speed);
    let current = |store: &ReplayStore| {
        let _ = store;
        gate_current(&shared.replay_gate, generation)
    };
    match consume_stream(&shared, &url, |shared, event, data| {
        match event {
            "snapshot" => match serde_json::from_str::<ReplaySnapshot>(data) {
                Ok(snapshot) => shared.update(|store| {
                    if !current(store) {
                        return;
                    }
                    if Some(snapshot.cursor.session_key) == store.session_key {
                        store.current_snapshot = Some(snapshot);
                        store.snapshot_loading = false;
                    }
                }),
                Err(error) => shared.update(|store| {
                    if current(store) {
                        store.snapshot_error = Some(error.to_string());
                    }
                }),
            },
            _ => {}
        }
    })
    .await
    {
        StreamOutcome::Ended => shared.update(|store| {
            // The server's `end`: the replay ran to completion.
            if gate_current(&shared.replay_gate, generation) {
                store.playing = false;
            }
        }),
        StreamOutcome::TransportError => shared.update(|store| {
            if gate_current(&shared.replay_gate, generation) {
                store.snapshot_error = Some("Replay stream disconnected.".to_string());
            }
        }),
    }
}

fn reconcile_sim_stream(shared: &Arc<Shared>) {
    let desired = shared.read(|store| {
        store
            .session_key
            .filter(|_| store.live_simulation_active)
    });
    let mut aux = shared.aux.lock().expect("aux lock");
    if aux.sim_stream.as_ref().map(|task| task.config) == desired {
        return;
    }
    if let Some(task) = aux.sim_stream.take() {
        task.handle.abort();
    }
    let generation = next_generation(&shared.sim_gate);
    let Some(key) = desired else { return };
    shared.update(|store| {
        store.live_simulation_connection = SimConnection::Connecting;
        store.snapshot_loading = true;
        store.snapshot_error = None;
    });
    let shared_task = Arc::clone(shared);
    let handle = shared.tokio.spawn(async move {
        run_sim_stream(shared_task, key, generation).await;
    });
    aux.sim_stream = Some(StreamTask { config: key, handle });
}

async fn run_sim_stream(shared: Arc<Shared>, key: i64, generation: u64) {
    let url = shared.api.live_simulation_stream_url(key);
    let gate_ok = || gate_current(&shared.sim_gate, generation);
    match consume_stream(&shared, &url, |shared, event, data| {
        if !gate_ok() {
            return;
        }
        match event {
            "metadata" => match serde_json::from_str::<ReplayMetadata>(data) {
                Ok(metadata) => shared.update(|store| store.live_metadata = Some(metadata)),
                Err(error) => {
                    shared.update(|store| store.snapshot_error = Some(error.to_string()))
                }
            },
            "snapshot" => match serde_json::from_str::<ReplaySnapshot>(data) {
                Ok(snapshot) => shared.update(|store| {
                    if Some(snapshot.cursor.session_key) == store.session_key {
                        store.time = snapshot.cursor.t;
                        store.current_snapshot = Some(snapshot);
                        store.live_simulation_connection = SimConnection::Connected;
                        store.snapshot_loading = false;
                    }
                }),
                Err(error) => shared.update(|store| {
                    store.snapshot_error = Some(error.to_string());
                    store.live_simulation_connection = SimConnection::Disconnected;
                }),
            },
            _ => {}
        }
    })
    .await
    {
        StreamOutcome::Ended => shared.update(|store| {
            if gate_ok() {
                store.live_simulation_active = false;
                store.live_simulation_connection = SimConnection::Idle;
                store.clear_live_runtime_resources();
            }
        }),
        StreamOutcome::TransportError => shared.update(|store| {
            if gate_ok() {
                store.snapshot_error = Some("Live simulation stream disconnected.".to_string());
                store.live_simulation_connection = SimConnection::Disconnected;
            }
        }),
    }
}

fn reconcile_live_stream(shared: &Arc<Shared>) {
    let desired = shared.read(|store| {
        store
            .session_key
            .filter(|_| store.live_active)
            .map(|key| (key, store.live_reconnect_nonce))
    });
    let mut aux = shared.aux.lock().expect("aux lock");
    if aux.live_stream.as_ref().map(|task| task.config) == desired {
        return;
    }
    if let Some(task) = aux.live_stream.take() {
        task.handle.abort();
    }
    let generation = next_generation(&shared.live_gate);
    let Some(config) = desired else { return };
    shared.update(|store| {
        store.live_connection = LiveConnection::Connecting;
        store.snapshot_loading = true;
        store.snapshot_error = None;
    });
    let shared_task = Arc::clone(shared);
    let handle = shared.tokio.spawn(async move {
        run_live_stream(shared_task, config.0, generation).await;
    });
    aux.live_stream = Some(StreamTask { config, handle });
}

async fn run_live_stream(shared: Arc<Shared>, key: i64, generation: u64) {
    let url = shared.api.live_stream_url(key);
    let gate_ok = || gate_current(&shared.live_gate, generation);
    let outcome = consume_stream(&shared, &url, |shared, event, data| {
        if !gate_ok() {
            return;
        }
        match event {
            "metadata" => match serde_json::from_str::<ReplayMetadata>(data) {
                Ok(metadata) => shared.update(|store| store.live_metadata = Some(metadata)),
                Err(error) => {
                    shared.update(|store| store.snapshot_error = Some(error.to_string()))
                }
            },
            "snapshot" => match serde_json::from_str::<ReplaySnapshot>(data) {
                Ok(snapshot) => {
                    let session_key = snapshot.cursor.session_key;
                    let applied = shared.update(|store| {
                        if Some(session_key) == store.session_key {
                            store.time = snapshot.cursor.t;
                            store.current_snapshot = Some(snapshot);
                            store.live_connection = LiveConnection::Connected;
                            store.snapshot_loading = false;
                            store.openf1_live_reconnect_attempts = 0;
                            true
                        } else {
                            false
                        }
                    });
                    if applied {
                        let shared = Arc::clone(shared);
                        shared.tokio.clone().spawn(async move {
                            refresh_openf1_live_status(&shared, session_key, generation).await;
                        });
                    }
                }
                Err(error) => shared.update(|store| {
                    store.snapshot_error = Some(error.to_string());
                    store.live_connection = LiveConnection::Disconnected;
                }),
            },
            "event" => match serde_json::from_str::<ReplayEvent>(data) {
                Ok(event) => shared.update(|store| {
                    if event.t <= store.time {
                        store.push_live_event(event);
                    }
                }),
                Err(error) => {
                    shared.update(|store| store.snapshot_error = Some(error.to_string()))
                }
            },
            "error" => {
                // A server-sent error event: surface it and reconnect with backoff.
                if let Some(message) = playback::server_sent_error_message(Some(data)) {
                    shared.update(|store| {
                        store.snapshot_error = Some(message.clone());
                        store.snapshot_loading = false;
                    });
                    let shared_refresh = Arc::clone(shared);
                    shared.tokio.clone().spawn(async move {
                        refresh_openf1_live_status(&shared_refresh, key, generation).await;
                    });
                    schedule_openf1_live_reconnect(shared, Some(message));
                }
            }
            _ => {}
        }
    })
    .await;

    if !gate_ok() {
        return;
    }
    let still_live = shared.read(|store| store.live_active);
    match outcome {
        StreamOutcome::Ended => {
            let reconnecting =
                shared.read(|store| store.live_connection == LiveConnection::Reconnecting);
            if reconnecting {
                // The server closed the stream after its error event; the scheduled
                // reconnect owns the next step.
                return;
            }
            shared.update(|store| {
                let return_key = playback::session_key_after_live_stops(
                    store.session_key,
                    store.return_session_key_after_live,
                );
                store.live_active = false;
                store.live_availability = LiveAvailability::Inactive;
                store.live_connection = LiveConnection::Idle;
                store.clear_live_runtime_resources();
                store.session_key = return_key;
                store.return_session_key_after_live = None;
                store.live_availability_message = Some("Live session ended.".to_string());
            });
            clear_openf1_live_reconnect(&shared);
        }
        StreamOutcome::TransportError => {
            if still_live {
                schedule_openf1_live_reconnect(&shared, None);
            }
        }
    }
}

async fn refresh_openf1_live_status(shared: &Arc<Shared>, key: i64, generation: u64) {
    {
        let mut aux = shared.aux.lock().expect("aux lock");
        if aux
            .last_status_refresh
            .is_some_and(|last| last.elapsed() < LIVE_STATUS_REFRESH_MIN_INTERVAL)
        {
            return;
        }
        aux.last_status_refresh = Some(Instant::now());
    }
    let status = shared.api.live_status(key).await.ok();
    if gate_current(&shared.live_gate, generation) {
        shared.update(|store| {
            if playback::should_apply_live_resource_result(
                status.as_ref().map(|status| status.session_key),
                store.session_key,
                store.live_active,
            ) {
                store.live_status = status;
            }
        });
    }
    let geometry = shared.api.live_track_geometry(key).await.ok();
    if gate_current(&shared.live_gate, generation) {
        shared.update(|store| {
            if playback::should_apply_live_resource_result(
                geometry.as_ref().map(|geometry| geometry.session_key),
                store.session_key,
                store.live_active,
            ) {
                store.live_geometry = geometry;
            }
        });
    }
}

fn schedule_openf1_live_reconnect(shared: &Arc<Shared>, reason: Option<String>) {
    clear_openf1_live_reconnect(shared);
    let delay_ms = shared.update(|store| {
        store.openf1_live_reconnect_attempts += 1;
        let exponent = (store.openf1_live_reconnect_attempts - 1).min(4);
        let delay_ms = (1000u64 * 2u64.pow(exponent)).min(15_000);
        store.live_connection = LiveConnection::Reconnecting;
        let prefix = reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .unwrap_or("OpenF1 live stream disconnected.");
        store.snapshot_error = Some(format!(
            "{prefix} Reconnecting in {}s.",
            (delay_ms as f64 / 1000.0).round() as u64
        ));
        delay_ms
    });
    let shared_timer = Arc::clone(shared);
    let handle = shared.tokio.spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        shared_timer.update(|store| {
            if store.live_active {
                store.live_reconnect_nonce += 1;
            }
        });
    });
    shared.aux.lock().expect("aux lock").reconnect_timer = Some(handle);
}

fn clear_openf1_live_reconnect(shared: &Arc<Shared>) {
    if let Some(timer) = shared.aux.lock().expect("aux lock").reconnect_timer.take() {
        timer.abort();
    }
}

// ---- Stream transport ----

enum StreamOutcome {
    /// The response body completed normally: the server sent its (dataless) `end`
    /// event and closed. Browsers never dispatch that event either — end-of-body is
    /// the real signal.
    Ended,
    TransportError,
}

async fn consume_stream(
    shared: &Arc<Shared>,
    url: &str,
    mut on_event: impl FnMut(&Arc<Shared>, &str, &str),
) -> StreamOutcome {
    let response = match shared.api.open_stream(url).await {
        Ok(response) => response,
        Err(_) => return StreamOutcome::TransportError,
    };
    let mut parser = SseParser::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(bytes) => {
                for event in parser.push(&bytes) {
                    on_event(shared, &event.event, &event.data);
                }
            }
            Err(_) => return StreamOutcome::TransportError,
        }
    }
    StreamOutcome::Ended
}
