//! The session selector's state machine: the port of the signals, resources and
//! effects inside `frontend/src/components/SessionSelector.tsx`.
//!
//! Same architecture as [`crate::store::runtime`]: plain state behind a mutex, one
//! reconciler task that re-runs the component's `createEffect` bodies after every
//! change (its own or the replay store's, whose active session it mirrors), and
//! request-id fencing for the ingest flow.

use std::sync::{Arc, Mutex, MutexGuard};

use interval_backend::domain::{IngestResponse, Meeting, Season, Session, SessionReadiness};
use tokio::sync::{mpsc, watch};

use crate::api_client::ApiClient;
use crate::session_readiness::{
    SessionActionState, can_open_session_after_ingest, can_open_session_from_cache,
    can_start_session_action, should_clear_transient_session_action,
};
use crate::session_selection::{
    self, SelectedSessionKeys, next_meeting_selection, next_season_selection,
    next_session_selection, should_sync_active_session_selection,
};
use crate::store::runtime::StoreRuntime;

/// A keyed resource: the loaded value plus the key it was loaded for.
#[derive(Debug, Clone)]
pub struct Keyed<T, K> {
    pub key: Option<K>,
    pub value: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T, K> Default for Keyed<T, K> {
    fn default() -> Self {
        Self {
            key: None,
            value: None,
            loading: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelectorState {
    pub selected_season: Option<i32>,
    pub selected_meeting: Option<i64>,
    pub selected_session: Option<i64>,
    pub ingest_state: SessionActionState,
    pub ingest_error: Option<String>,
    pub last_ingest: Option<IngestResponse>,
    /// Once the user drives the selects, the active session stops steering them
    /// (until they navigate back onto it).
    pub has_user_browsed: bool,

    pub seasons: Keyed<Vec<Season>, ()>,
    pub meetings: Keyed<Vec<Meeting>, i32>,
    pub sessions: Keyed<Vec<SessionReadiness>, i64>,

    session_intent: u64,
    meeting_intent: u64,
    handled_session_intent: u64,
    handled_meeting_intent: u64,
    session_action_request_id: u64,
    sessions_refetch_nonce: u64,
    sessions_refetch_done: u64,
    last_selected_session: Option<i64>,
    last_active_session_key: Option<i64>,
}

impl SelectorState {
    pub fn selected_readiness(&self) -> Option<&SessionReadiness> {
        session_selection::readiness_for_session(
            self.sessions_for_selection(),
            self.selected_session,
        )
    }

    pub fn selected_session_label(&self) -> Option<String> {
        session_selection::selected_session_label(self.selected_readiness())
    }

    pub fn ingest_state(&self) -> SessionActionState {
        self.ingest_state
    }

    /// The sessions list, only while it belongs to the currently selected meeting.
    fn sessions_for_selection(&self) -> Option<&[SessionReadiness]> {
        (self.sessions.key == self.selected_meeting && self.selected_meeting.is_some())
            .then(|| self.sessions.value.as_deref())
            .flatten()
    }

    fn meetings_for_selection(&self) -> Option<&[Meeting]> {
        (self.meetings.key == self.selected_season && self.selected_season.is_some())
            .then(|| self.meetings.value.as_deref())
            .flatten()
    }

    pub fn season_options(&self) -> Vec<session_selection::SessionSelectOption> {
        session_selection::season_options(self.seasons.value.as_deref())
    }

    pub fn meeting_options(&self) -> Vec<session_selection::SessionSelectOption> {
        session_selection::meeting_options(self.meetings_for_selection())
    }

    pub fn session_options(&self) -> Vec<session_selection::SessionSelectOption> {
        session_selection::session_options(self.sessions_for_selection())
    }

    pub fn discovery_failed(&self) -> bool {
        self.meetings.error.is_some() || self.sessions.error.is_some()
    }

    pub fn no_race_session_for_meeting(&self) -> bool {
        !self.sessions.loading
            && self.selected_meeting.is_some()
            && self
                .sessions_for_selection()
                .is_some_and(|sessions| sessions.is_empty())
    }
}

struct Shared {
    state: Mutex<SelectorState>,
    api: ApiClient,
    store: Arc<StoreRuntime>,
    tokio: tokio::runtime::Handle,
    changed: watch::Sender<u64>,
    repaint: mpsc::UnboundedSender<()>,
}

impl Shared {
    fn update<R>(&self, f: impl FnOnce(&mut SelectorState) -> R) -> R {
        let result = {
            let mut state = self.state.lock().expect("selector lock");
            f(&mut state)
        };
        self.changed.send_modify(|version| *version += 1);
        let _ = self.repaint.send(());
        result
    }

    fn read<R>(&self, f: impl FnOnce(&SelectorState) -> R) -> R {
        f(&self.state.lock().expect("selector lock"))
    }
}

pub struct SelectorRuntime {
    shared: Arc<Shared>,
}

impl SelectorRuntime {
    pub fn start(
        api: ApiClient,
        store: Arc<StoreRuntime>,
        repaint: mpsc::UnboundedSender<()>,
    ) -> Arc<Self> {
        let (changed, _) = watch::channel(0u64);
        let shared = Arc::new(Shared {
            state: Mutex::new(SelectorState::default()),
            api,
            store,
            tokio: tokio::runtime::Handle::current(),
            changed,
            repaint,
        });
        shared.tokio.spawn(reconciler_task(Arc::clone(&shared)));
        Arc::new(Self { shared })
    }

    pub fn state(&self) -> MutexGuard<'_, SelectorState> {
        self.shared.state.lock().expect("selector lock")
    }

    pub fn choose_season(&self, season: i32) {
        self.shared.store.clear_active_session(None);
        self.shared.update(|state| {
            state.has_user_browsed = true;
            state.session_action_request_id += 1;
            state.selected_season = Some(season);
            state.selected_meeting = None;
            state.selected_session = None;
        });
    }

    pub fn choose_meeting(&self, meeting: i64) {
        self.shared.store.clear_active_session(None);
        self.shared.update(|state| {
            state.has_user_browsed = true;
            state.session_action_request_id += 1;
            state.selected_meeting = Some(meeting);
            state.selected_session = None;
            state.meeting_intent += 1;
        });
    }

    pub fn choose_session(&self, session: i64) {
        self.shared.update(|state| {
            state.has_user_browsed = true;
            state.selected_session = Some(session);
            state.session_intent += 1;
        });
    }

    /// The OPEN/INGEST button.
    pub fn open_selected(&self) {
        let shared = Arc::clone(&self.shared);
        self.shared.tokio.spawn(async move {
            open_selected(&shared).await;
        });
    }
}

async fn reconciler_task(shared: Arc<Shared>) {
    let mut own_rx = shared.changed.subscribe();
    let mut store_rx = shared.store.subscribe();
    loop {
        own_rx.borrow_and_update();
        store_rx.borrow_and_update();
        reconcile(&shared);
        tokio::select! {
            changed = own_rx.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            changed = store_rx.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

fn reconcile(shared: &Arc<Shared>) {
    run_active_session_sync_effect(shared);
    run_selection_normalization_effects(shared);
    run_clear_transient_action_effect(shared);
    run_meeting_intent_effect(shared);
    run_session_intent_effect(shared);
    reconcile_seasons(shared);
    reconcile_meetings(shared);
    reconcile_sessions(shared);
}

/// Mirrors the active (open) session into the selects, unless the user is browsing
/// somewhere else.
fn run_active_session_sync_effect(shared: &Arc<Shared>) {
    let active: Option<Session> = {
        let store = shared.store.state();
        store.display_metadata().map(|meta| meta.session.clone())
    };
    let Some(active) = active else { return };
    let needs_update = shared.read(|state| {
        if state.has_user_browsed && state.selected_session != Some(active.session_key) {
            return None;
        }
        let selected = SelectedSessionKeys {
            season: state.selected_season,
            meeting: state.selected_meeting,
            session: state.selected_session,
        };
        Some(should_sync_active_session_selection(
            &active,
            &selected,
            state.last_active_session_key,
        ))
    });
    match needs_update {
        Some(true) => shared.update(|state| {
            state.selected_season = Some(active.year);
            state.selected_meeting = Some(active.meeting_key);
            state.selected_session = Some(active.session_key);
            state.last_active_session_key = Some(active.session_key);
        }),
        Some(false) => {
            let stale = shared
                .read(|state| state.last_active_session_key != Some(active.session_key));
            if stale {
                shared
                    .update(|state| state.last_active_session_key = Some(active.session_key));
            }
        }
        None => {}
    }
}

fn run_selection_normalization_effects(shared: &Arc<Shared>) {
    let changes = shared.read(|state| {
        let season =
            next_season_selection(state.seasons.value.as_deref(), state.selected_season, None);
        let meeting = next_meeting_selection(
            state.meetings_for_selection(),
            state.selected_meeting,
            None,
        );
        let session = next_session_selection(
            state.sessions_for_selection(),
            state.selected_session,
            None,
        );
        (season != state.selected_season)
            .then_some(season)
            .map(|s| ("season", s.map(|v| v as i64)))
            .into_iter()
            .chain(
                (meeting != state.selected_meeting)
                    .then_some(("meeting", meeting)),
            )
            .chain(
                (session != state.selected_session)
                    .then_some(("session", session)),
            )
            .collect::<Vec<_>>()
    });
    if changes.is_empty() {
        return;
    }
    shared.update(|state| {
        for (which, value) in changes {
            match which {
                "season" => state.selected_season = value.map(|v| v as i32),
                "meeting" => state.selected_meeting = value,
                "session" => state.selected_session = value,
                _ => unreachable!(),
            }
        }
    });
}

fn run_clear_transient_action_effect(shared: &Arc<Shared>) {
    let should = shared.read(|state| {
        (
            should_clear_transient_session_action(
                state.last_selected_session,
                state.selected_session,
                state.ingest_state,
            ),
            state.last_selected_session != state.selected_session,
        )
    });
    match should {
        (true, _) => shared.update(|state| {
            state.ingest_state = SessionActionState::Idle;
            state.ingest_error = None;
            state.last_ingest = None;
            state.last_selected_session = state.selected_session;
        }),
        (false, true) => {
            shared.update(|state| state.last_selected_session = state.selected_session)
        }
        (false, false) => {}
    }
}

/// A meeting choice auto-opens its (auto-selected) session once the list loads.
fn run_meeting_intent_effect(shared: &Arc<Shared>) {
    let fire = shared.read(|state| {
        state.meeting_intent != 0
            && state.meeting_intent != state.handled_meeting_intent
            && !state.sessions.loading
            && state.selected_session.is_some()
            && state.selected_readiness().is_some()
    });
    if fire {
        shared.update(|state| {
            state.handled_meeting_intent = state.meeting_intent;
            state.session_intent += 1;
        });
    }
}

fn run_session_intent_effect(shared: &Arc<Shared>) {
    let fire = shared.read(|state| {
        state.session_intent != 0
            && state.session_intent != state.handled_session_intent
            && state.selected_session.is_some()
            && state.selected_readiness().is_some()
    });
    if fire {
        shared.update(|state| state.handled_session_intent = state.session_intent);
        let shared = Arc::clone(shared);
        shared.tokio.clone().spawn(async move {
            open_selected(&shared).await;
        });
    }
}

fn reconcile_seasons(shared: &Arc<Shared>) {
    let start = shared.read(|state| {
        state.seasons.key.is_none() && !state.seasons.loading
    });
    if !start {
        return;
    }
    shared.update(|state| {
        state.seasons.loading = true;
    });
    let shared = Arc::clone(shared);
    shared.tokio.clone().spawn(async move {
        let result = shared.api.seasons().await;
        shared.update(|state| {
            state.seasons.loading = false;
            state.seasons.key = Some(());
            match result {
                Ok(seasons) => state.seasons.value = Some(seasons),
                Err(error) => state.seasons.error = Some(error.to_string()),
            }
        });
    });
}

fn reconcile_meetings(shared: &Arc<Shared>) {
    let desired = shared.read(|state| state.selected_season);
    let Some(season) = desired else { return };
    let start = shared.read(|state| state.meetings.key != Some(season) && !state.meetings.loading);
    if !start {
        return;
    }
    shared.update(|state| {
        state.meetings.loading = true;
        state.meetings.error = None;
    });
    let shared = Arc::clone(shared);
    shared.tokio.clone().spawn(async move {
        let result = shared.api.meetings(season).await;
        shared.update(|state| {
            // A newer season choice may have superseded this fetch.
            if state.selected_season != Some(season) {
                state.meetings.loading = false;
                return;
            }
            state.meetings.loading = false;
            state.meetings.key = Some(season);
            match result {
                Ok(meetings) => {
                    state.meetings.value = Some(meetings);
                    state.meetings.error = None;
                }
                Err(error) => state.meetings.error = Some(error.to_string()),
            }
        });
    });
}

fn reconcile_sessions(shared: &Arc<Shared>) {
    let desired = shared.read(|state| state.selected_meeting.map(|m| (m, state.sessions_refetch_nonce)));
    let Some((meeting, nonce)) = desired else { return };
    let start = shared.read(|state| {
        (state.sessions.key != Some(meeting) || nonce != state.sessions_refetch_done)
            && !state.sessions.loading
    });
    if !start {
        return;
    }
    shared.update(|state| {
        state.sessions.loading = true;
        state.sessions.error = None;
    });
    let shared = Arc::clone(shared);
    shared.tokio.clone().spawn(async move {
        let result = shared.api.sessions(meeting).await;
        shared.update(|state| {
            state.sessions.loading = false;
            if state.selected_meeting != Some(meeting) {
                return;
            }
            state.sessions.key = Some(meeting);
            state.sessions_refetch_done = nonce;
            match result {
                Ok(sessions) => {
                    state.sessions.value = Some(sessions);
                    state.sessions.error = None;
                }
                Err(error) => state.sessions.error = Some(error.to_string()),
            }
        });
    });
}

async fn open_selected(shared: &Arc<Shared>) {
    let Some((key, request_id)) = shared.update(|state| {
        let key = state.selected_session?;
        state.session_action_request_id += 1;
        Some((key, state.session_action_request_id))
    }) else {
        return;
    };
    // props.onSessionIntent(key): tear down the current session unless it is this one.
    shared.store.clear_active_session(Some(key));

    let is_current = |state: &SelectorState| {
        state.session_action_request_id == request_id && state.selected_session == Some(key)
    };

    let can_start = shared.read(|state| {
        can_start_session_action(state.selected_readiness())
    });
    if !can_start {
        shared.update(|state| {
            state.ingest_state = SessionActionState::Failed;
            state.ingest_error = Some(
                state
                    .selected_readiness()
                    .and_then(|entry| entry.support_reason.clone())
                    .unwrap_or_else(|| "Selected session is unavailable.".to_string()),
            );
        });
        return;
    }

    let from_cache = shared.update(|state| {
        state.ingest_state = SessionActionState::Checking;
        state.ingest_error = None;
        can_open_session_from_cache(state.selected_readiness())
    });
    if from_cache {
        let proceed = shared.read(|state| is_current(state));
        if !proceed {
            return;
        }
        shared.update(|state| state.ingest_state = SessionActionState::OpeningCache);
        shared.store.open_session(key);
        shared.update(|state| {
            if is_current(state) {
                state.ingest_error = None;
                state.last_ingest = None;
                state.ingest_state = SessionActionState::Idle;
            }
        });
        return;
    }

    shared.update(|state| {
        state.ingest_state = SessionActionState::Ingesting;
        state.ingest_error = None;
    });
    match shared.api.ingest(key).await {
        Ok(response) => {
            let proceed = shared.update(|state| {
                if !is_current(state) {
                    return false;
                }
                state.last_ingest = Some(response.clone());
                // refetchSessions(): refresh the readiness badge for the new state.
                state.sessions_refetch_nonce += 1;
                true
            });
            if !proceed {
                return;
            }
            if can_open_session_after_ingest(&response) {
                shared.update(|state| state.ingest_state = SessionActionState::OpeningReplay);
                shared.store.open_session(key);
                shared.update(|state| {
                    if is_current(state) {
                        state.ingest_state = SessionActionState::Idle;
                    }
                });
            } else {
                shared.update(|state| {
                    state.ingest_error = Some(
                        response
                            .error
                            .clone()
                            .unwrap_or_else(|| "Ingest did not produce replay frames.".to_string()),
                    );
                    state.ingest_state = SessionActionState::Failed;
                });
            }
        }
        Err(error) => {
            shared.update(|state| {
                if !is_current(state) {
                    return;
                }
                state.ingest_error = Some(error.to_string());
                state.sessions_refetch_nonce += 1;
                state.ingest_state = SessionActionState::Failed;
            });
        }
    }
}
