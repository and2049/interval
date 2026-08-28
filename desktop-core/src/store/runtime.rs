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

