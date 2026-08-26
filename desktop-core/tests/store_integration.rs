//! End-to-end store test against a real embedded backend: the same axum server the
//! desktop app runs in-process, seeded with the demo session, driven through the
//! store's public intents. This is the closest thing to the frontend running against
//! `cargo run -p interval-backend` — the HTTP contract, the SSE streams, and the
//! store's orchestration all get exercised for real.

use std::time::Duration;

use interval_backend::server::{self, ServeOptions};
use interval_desktop_core::api_client::ApiClient;
use interval_desktop_core::selector::SelectorRuntime;
use interval_desktop_core::session_keys::DEMO_SESSION_KEY;
use interval_desktop_core::store::runtime::StoreRuntime;
use interval_desktop_core::store::{ReplayStore, SimConnection};

async fn start_test_server(tag: &str) -> ApiClient {
    let dir = std::env::temp_dir().join(format!(
        "interval-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let database_url = format!("sqlite://{}", dir.join("test.db").display());
    let bound = server::serve(
        ServeOptions {
            bind: "127.0.0.1:0".parse().unwrap(),
            database_url,
            static_dir: None,
            enable_settings_api: false,
        },
        std::future::pending(),
    )
    .await
    .expect("embedded server starts");
    ApiClient::new(format!("http://{}", bound.addr))
}

async fn wait_for(
    runtime: &StoreRuntime,
    what: &str,
    mut predicate: impl FnMut(&ReplayStore) -> bool,
) {
    for _ in 0..600 {
        if predicate(&runtime.state()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let state = runtime.state();
    panic!(
        "timed out waiting for {what}; session_key={:?} metadata_key={:?} metadata_err={:?} \
         snapshot_loading={} snapshot_err={:?} playing={} time={}",
        state.session_key,
        state.metadata.key,
        state.metadata.error,
        state.snapshot_loading,
        state.snapshot_error,
        state.playing,
        state.time,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn store_drives_the_demo_session_end_to_end() {
    let api = start_test_server("store-test").await;
    let (repaint_tx, mut repaint_rx) = tokio::sync::mpsc::unbounded_channel();
    let runtime = StoreRuntime::start(api, None, Box::new(|_| {}), repaint_tx);

    // Opening the demo session fetches metadata + geometry + events, initializes the
    // cursor to the race start, and loads the paused frame.
    runtime.open_session(DEMO_SESSION_KEY);
    wait_for(&runtime, "metadata", |state| {
        state.active_metadata().is_some()
    })
    .await;
    wait_for(&runtime, "initial paused snapshot", |state| {
        state.active_snapshot().is_some()
    })
    .await;
    let (start_time, max_t) = {
        let state = runtime.state();
        let meta = state.active_metadata().unwrap();
        assert!(state.time >= meta.min_t);
        (state.time, meta.max_t)
    };

    // Play: the replay SSE stream drives the snapshot; the local ticker drives time.
    runtime.set_playing(true);
    wait_for(&runtime, "time to advance during playback", |state| {
        state.playing && state.time > start_time
    })
    .await;
    wait_for(&runtime, "a streamed snapshot", |state| {
        state
            .active_snapshot()
            .is_some_and(|snapshot| snapshot.cursor.t > start_time)
    })
    .await;

    // Pause and seek: the paused-frame effect fetches the quantized frame. The demo
    // session's frame grid is coarse (frame_step 60s), so seek onto a frame boundary
    // and expect exactly that frame back.
    runtime.set_playing(false);
    let frame_step = {
        let state = runtime.state();
        state.active_metadata().unwrap().frame_step_seconds
    };
    let seek_target = (start_time + frame_step).min(max_t);
    runtime.seek(seek_target);
    wait_for(&runtime, "the sought paused frame", |state| {
        !state.playing
            && state
                .active_snapshot()
                .is_some_and(|snapshot| (snapshot.cursor.t - seek_target).abs() < 1e-6)
    })
    .await;

    // Live simulation: start, receive a snapshot over its stream, stop.
    runtime.toggle_live_simulation();
    wait_for(&runtime, "live simulation connected", |state| {
        state.live_simulation_active
            && state.live_simulation_connection == SimConnection::Connected
    })
    .await;
    runtime.toggle_live_simulation();
    wait_for(&runtime, "live simulation stopped", |state| {
        !state.live_simulation_active
            && state.live_simulation_connection == SimConnection::Idle
    })
    .await;

    // The UI repaint channel actually carried messages.
    assert!(repaint_rx.try_recv().is_ok() || !repaint_rx.is_empty());
}

/// The selector's browse→auto-open chain: choosing the demo meeting auto-selects its
/// session and opens it from cache without a click on OPEN.
#[tokio::test(flavor = "multi_thread")]
async fn selector_auto_opens_the_demo_session() {
    let api = start_test_server("selector-test").await;
    let (repaint_tx, _repaint_rx) = tokio::sync::mpsc::unbounded_channel();
    let store = StoreRuntime::start(api.clone(), None, Box::new(|_| {}), repaint_tx.clone());
    let selector = SelectorRuntime::start(api, std::sync::Arc::clone(&store), repaint_tx);

    for _ in 0..600 {
        if selector.state().seasons.value.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        selector.state().seasons.value.is_some(),
        "seasons never loaded"
    );

    selector.choose_season(2025);
    for _ in 0..600 {
        if selector
            .state()
            .meeting_options()
            .iter()
            .any(|option| option.value == 1276)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    selector.choose_meeting(1276);
    wait_for(&store, "the demo session to auto-open", |state| {
        state.session_key == Some(DEMO_SESSION_KEY) && state.active_metadata().is_some()
    })
    .await;
    let selector_state = selector.state();
    assert_eq!(selector_state.selected_session, Some(DEMO_SESSION_KEY));
    assert!(!selector_state.discovery_failed());
}
