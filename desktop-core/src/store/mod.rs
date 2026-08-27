//! The replay store: the state half of `frontend/src/stores/replay.ts`.
//!
//! `ReplayStore` is plain data plus synchronous transitions and queries — everything
//! here is headlessly testable. The async half (HTTP fetches, SSE streams, tickers,
//! reconnect timers) lives in [`runtime`], which mutates this state under a mutex and
//! notifies the UI to repaint. The generation counters (`*_request_id`) are the port of
//! the frontend's closure counters: every async family captures the counter at spawn
//! and its continuation applies results only while the counter is unchanged.

pub mod runtime;

use interval_backend::domain::{
    LiveAvailability, LiveChannelHealth, LiveSessionStatus, ReplayEvent, ReplayMetadata,
    ReplaySnapshot, TrackGeometry,
};

use crate::playback::{
    self, active_replay_metadata, active_replay_snapshot, active_track_geometry,
    replay_resource_session_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveConnection {
    Idle,
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimConnection {
    Idle,
    Connecting,
    Connected,
    Disconnected,
}

/// The port of a Solid `createResource`: the latest resolved value (kept while a newer
/// fetch is in flight, as Solid does), the in-flight flag, and the error of the most
/// recent settle. `key` is the session the value/error belongs to.
#[derive(Debug, Clone)]
pub struct Resource<T> {
    pub key: Option<i64>,
    pub value: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T> Default for Resource<T> {
    fn default() -> Self {
        Self {
            key: None,
            value: None,
            loading: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReplayStore {
    pub session_key: Option<i64>,
    pub playing: bool,
    pub speed: f64,
    pub time: f64,
    pub stream_start_time: f64,
    pub current_snapshot: Option<ReplaySnapshot>,

    pub metadata: Resource<ReplayMetadata>,
    pub track_geometry: Resource<TrackGeometry>,
    pub events: Resource<Vec<ReplayEvent>>,

    pub live_metadata: Option<ReplayMetadata>,
    pub live_geometry: Option<TrackGeometry>,
    pub live_status: Option<LiveSessionStatus>,
    pub live_events: Vec<ReplayEvent>,
    pub live_availability: LiveAvailability,
    pub live_availability_message: Option<String>,
    pub live_availability_checking: bool,
    pub live_active: bool,
    pub live_connection: LiveConnection,
    pub live_reconnect_nonce: u64,
    pub live_simulation_active: bool,
    pub live_simulation_connection: SimConnection,

    pub snapshot_loading: bool,
    pub snapshot_error: Option<String>,

    // The frontend store's closure variables.
    pub(crate) snapshot_request_id: u64,
    pub(crate) initialized_session_key: Option<i64>,
    pub(crate) openf1_live_reconnect_attempts: u32,
    pub(crate) openf1_live_check_request_id: u64,
    pub(crate) openf1_live_start_request_id: u64,
    pub(crate) live_simulation_start_request_id: u64,
    pub(crate) return_session_key_after_live: Option<i64>,
    /// Bumped by `open_session` on the already-open key, where the frontend called the
    /// resources' `refetch()`; the resource supervisor watches this alongside the key.
    pub(crate) resources_refetch_nonce: u64,
}

impl ReplayStore {
    pub fn new(stored_session_key: Option<i64>) -> Self {
        Self {
            session_key: stored_session_key,
            playing: false,
            speed: 1.0,
            time: 0.0,
            stream_start_time: 0.0,
            current_snapshot: None,
            metadata: Resource::default(),
            track_geometry: Resource::default(),
            events: Resource::default(),
            live_metadata: None,
            live_geometry: None,
            live_status: None,
            live_events: Vec::new(),
            live_availability: LiveAvailability::Inactive,
            live_availability_message: None,
            live_availability_checking: false,
            live_active: false,
            live_connection: LiveConnection::Idle,
            live_reconnect_nonce: 0,
            live_simulation_active: false,
            live_simulation_connection: SimConnection::Idle,
            snapshot_loading: false,
            snapshot_error: None,
            snapshot_request_id: 0,
            initialized_session_key: None,
            openf1_live_reconnect_attempts: 0,
            openf1_live_check_request_id: 0,
            openf1_live_start_request_id: 0,
            live_simulation_start_request_id: 0,
            return_session_key_after_live: stored_session_key,
            resources_refetch_nonce: 0,
        }
    }

    // ---- Queries (the derived signals) ----

    pub fn active_metadata(&self) -> Option<&ReplayMetadata> {
        active_replay_metadata(self.session_key, self.metadata.value.as_ref())
    }

    /// Live metadata wins while a live/sim runtime owns the session.
    pub fn display_metadata(&self) -> Option<&ReplayMetadata> {
        if (self.live_active || self.live_simulation_active)
            && self
                .live_metadata
                .as_ref()
                .is_some_and(|live| Some(live.session.session_key) == self.session_key)
        {
            self.live_metadata.as_ref()
        } else {
            self.active_metadata()
        }
    }

    pub fn active_snapshot(&self) -> Option<&ReplaySnapshot> {
        active_replay_snapshot(self.session_key, self.current_snapshot.as_ref())
    }

    pub fn active_geometry(&self) -> Option<&TrackGeometry> {
        if self.live_active
            && self
                .live_geometry
                .as_ref()
                .is_some_and(|geometry| Some(geometry.session_key) == self.session_key)
        {
            self.live_geometry.as_ref()
        } else {
            active_track_geometry(self.session_key, self.track_geometry.value.as_ref())
        }
    }

    pub fn active_geometry_error(&self) -> Option<&str> {
        if playback::should_hide_historical_resource_error(self.live_active) {
            return None;
        }
        playback::active_resource_error(
            self.session_key,
            self.resource_session_key(),
            self.track_geometry.error.as_deref(),
        )
    }

    pub fn active_events(&self) -> &[ReplayEvent] {
        if self.live_active {
            &self.live_events
        } else if self.events.key == self.session_key && self.session_key.is_some() {
            self.events.value.as_deref().unwrap_or(&[])
        } else {
            &[]
        }
    }

    pub fn active_events_loading(&self) -> bool {
        if self.live_active {
            false
        } else {
            playback::active_resource_loading(
                self.session_key,
                self.resource_session_key(),
                self.events.loading,
            )
        }
    }

    pub fn active_events_error(&self) -> Option<&str> {
        if self.live_active {
            None
        } else {
            playback::active_resource_error(
                self.session_key,
                self.resource_session_key(),
                self.events.error.as_deref(),
            )
        }
    }

    pub fn live_channels(&self) -> &[LiveChannelHealth] {
        self.live_status
            .as_ref()
            .map(|status| status.channels.as_slice())
            .unwrap_or(&[])
    }

    pub fn live_transitioning(&self) -> bool {
        self.live_connection == LiveConnection::Connecting
            || self.live_connection == LiveConnection::Reconnecting
            || self.live_simulation_connection == SimConnection::Connecting
    }

    pub(crate) fn resource_session_key(&self) -> Option<i64> {
        replay_resource_session_key(self.session_key, self.metadata.value.as_ref())
    }

    // ---- Synchronous transitions (the user intents that never touch the network) ----

    /// `setPlaying`: ignored while a live runtime owns playback; starting playback
    /// re-bases the stream start time so the SSE stream opens at the current cursor.
    pub fn set_playing(&mut self, playing: bool) {
        if self.live_simulation_active || self.live_active {
            return;
        }
        if playing {
            self.stream_start_time = self.time;
        }
        self.playing = playing;
    }

    pub fn toggle_playing(&mut self) {
        let next = !self.playing;
        self.set_playing(next);
    }

    pub fn set_speed(&mut self, speed: f64) {
        if self.live_simulation_active || self.live_active {
            return;
        }
        self.speed = playback::normalize_replay_speed(speed);
        self.stream_start_time = self.time;
    }

    /// `seek` without the snapshot load; the runtime wrapper fetches the paused frame.
    pub(crate) fn seek_time(&mut self, next_time: f64) -> Option<f64> {
        if self.live_simulation_active || self.live_active {
            return None;
        }
        let max_t = self
            .active_metadata()
            .map(|metadata| metadata.max_t)
            .unwrap_or(f64::INFINITY);
        let clamped = playback::clamp_replay_time(next_time, max_t);
        self.time = clamped;
        self.stream_start_time = clamped;
        Some(clamped)
    }

    /// One 100ms playback tick. Returns true when the tick changed anything.
    pub fn apply_tick(&mut self, elapsed_seconds: f64) -> bool {
        let max_t = self.active_metadata().map(|metadata| metadata.max_t);
        let tick = playback::next_replay_tick(playback::NextReplayTickOptions {
            current_time: self.time,
            elapsed_seconds,
            speed: self.speed,
            max_t,
            playing: self.playing && !self.live_simulation_active && !self.live_active,
        });
        let changed = tick.time != self.time || tick.playing != self.playing;
        self.time = tick.time;
        self.playing = tick.playing;
        changed
    }

    pub(crate) fn clear_openf1_live_resources(&mut self) {
        self.live_metadata = None;
        self.live_geometry = None;
        self.live_status = None;
        self.live_events.clear();
    }

    pub(crate) fn clear_live_runtime_resources(&mut self) {
        self.clear_openf1_live_resources();
        self.current_snapshot = None;
    }

    /// The shared teardown of `clearActiveSession` and `openSession` (identical in the
    /// frontend up to what happens with the session key afterwards).
    pub(crate) fn reset_for_session_change(&mut self) {
        self.openf1_live_check_request_id += 1;
        self.openf1_live_start_request_id += 1;
        self.live_simulation_start_request_id += 1;
        self.live_availability_checking = false;
        self.playing = false;
        self.live_active = false;
        self.live_connection = LiveConnection::Idle;
        self.live_simulation_active = false;
        self.live_simulation_connection = SimConnection::Idle;
        self.clear_live_runtime_resources();
        self.time = 0.0;
        self.stream_start_time = 0.0;
        self.snapshot_error = None;
        self.initialized_session_key = None;
        self.return_session_key_after_live = None;
        self.snapshot_request_id += 1;
    }

    /// Dedupes by id and keeps the buffer sorted by `t` — the live `event` listener.
    pub(crate) fn push_live_event(&mut self, event: ReplayEvent) {
        if self.live_events.iter().any(|existing| existing.id == event.id) {
            return;
        }
        self.live_events.push(event);
        self.live_events
            .sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(session_key: i64) -> ReplayMetadata {
        serde_json::from_value(serde_json::json!({
            "session": {
                "session_key": session_key,
                "meeting_key": 1,
                "year": 2024,
                "name": "Race",
                "session_type": "race",
                "start_time": "2024-03-02T15:00:00Z",
                "end_time": "2024-03-02T17:00:00Z",
                "total_laps": 57
            },
            "duration_seconds": 100.0,
            "frame_step_seconds": 0.2,
            "total_frames": 500,
            "drivers": [],
            "min_t": 0.0,
            "max_t": 100.0,
            "race_start_t": 10.0
        }))
        .expect("test metadata builds")
    }

    fn store_with_metadata(session_key: i64) -> ReplayStore {
        let mut store = ReplayStore::new(Some(session_key));
        store.metadata.key = Some(session_key);
        store.metadata.value = Some(metadata(session_key));
        store
    }

    #[test]
    fn active_metadata_requires_matching_session() {
        let mut store = store_with_metadata(9472);
        assert!(store.active_metadata().is_some());
        store.session_key = Some(1);
        assert!(store.active_metadata().is_none());
    }

    #[test]
    fn set_playing_rebases_stream_start_and_is_inert_while_live() {
        let mut store = store_with_metadata(9472);
        store.time = 42.0;
        store.set_playing(true);
        assert!(store.playing);
        assert_eq!(store.stream_start_time, 42.0);

        store.live_active = true;
        store.set_playing(false);
        assert!(store.playing, "live mode ignores playback intents");
    }

    #[test]
    fn set_speed_normalizes_and_rebases() {
        let mut store = store_with_metadata(9472);
        store.time = 5.0;
        store.set_speed(3.0);
        assert_eq!(store.speed, 1.0);
        assert_eq!(store.stream_start_time, 5.0);
        store.set_speed(4.0);
        assert_eq!(store.speed, 4.0);
    }

    #[test]
    fn seek_clamps_to_metadata_max() {
        let mut store = store_with_metadata(9472);
        assert_eq!(store.seek_time(500.0), Some(100.0));
        assert_eq!(store.time, 100.0);
        assert_eq!(store.stream_start_time, 100.0);
    }

    #[test]
    fn tick_advances_and_stops_at_max() {
        let mut store = store_with_metadata(9472);
        store.time = 99.5;
        store.playing = true;
        store.speed = 1.0;
        assert!(store.apply_tick(1.0));
        assert_eq!(store.time, 100.0);
        assert!(!store.playing, "reaching max_t stops playback");
    }

    #[test]
    fn tick_is_inert_without_metadata() {
        let mut store = ReplayStore::new(Some(9472));
        store.playing = true;
        assert!(!store.apply_tick(1.0));
        assert_eq!(store.time, 0.0);
    }

    #[test]
    fn push_live_event_dedupes_by_id_and_sorts_by_t() {
        let mut store = ReplayStore::new(Some(1));
        let event = |id: &str, t: f64| -> ReplayEvent {
            serde_json::from_value(serde_json::json!({
                "id": id, "t": t, "kind": "race_control", "severity": "info",
                "driver_number": null, "message": "m", "source": "system", "payload": {}
            }))
            .expect("test event builds")
        };
        store.push_live_event(event("b", 2.0));
        store.push_live_event(event("a", 1.0));
        store.push_live_event(event("b", 2.0));
        let ids: Vec<&str> = store.live_events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn display_metadata_prefers_live_only_for_matching_session() {
        let mut store = store_with_metadata(9472);
        store.live_active = true;
        store.live_metadata = Some(metadata(999));
        assert_eq!(
            store
                .display_metadata()
                .map(|meta| meta.session.session_key),
            Some(9472),
            "live metadata for another session is ignored"
        );
        store.live_metadata = Some(metadata(9472));
        assert!(store.display_metadata().is_some());
    }

    #[test]
    fn reset_for_session_change_bumps_generations_and_clears_runtime() {
        let mut store = store_with_metadata(9472);
        store.playing = true;
        store.live_active = true;
        store.time = 55.0;
        let old_snapshot_generation = store.snapshot_request_id;
        store.reset_for_session_change();
        assert!(!store.playing);
        assert!(!store.live_active);
        assert_eq!(store.time, 0.0);
        assert_eq!(store.snapshot_request_id, old_snapshot_generation + 1);
        assert!(store.current_snapshot.is_none());
    }
}
