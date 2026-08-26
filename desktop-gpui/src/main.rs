//! interval — the GPUI desktop frontend.
//!
//! Embeds the axum backend in-process (see `embed`, Phase 1): the server binds
//! 127.0.0.1:0 on the tokio runtime below and the UI consumes it over HTTP + SSE,
//! exactly like the web frontend does, so the backend's HTTP contract stays the
//! single source of truth.
//!
//! The tokio runtime lives on the main function's stack and stays entered for the
//! lifetime of the UI, so backend tasks keep running on its threads while GPUI
//! blocks in `run()`.

#![windows_subsystem = "windows"]

mod assets;
mod embed;
mod persist;
mod theme;
mod views;

use gpui::{
    App, Bounds, Context, KeyBinding, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, size,
};
use gpui_platform::application;

actions!(
    interval,
    [
        Quit,
        TogglePlayback,
        SeekBackward,
        SeekForward,
        Hide,
        HideOthers,
        ShowAll,
        MinimizeWindow,
        ZoomWindow
    ]
);

/// The macOS menu bar. Not `#[cfg]`-gated: the caller branches on `cfg!` so this stays
/// type-checked on every platform, and `set_menus` is a no-op off macOS.
fn mac_menus() -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};
    vec![
        Menu {
            name: "interval".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Hide interval", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit interval", Quit),
            ],
        },
        Menu {
            name: "Window".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Minimize", MinimizeWindow),
                MenuItem::action("Zoom", ZoomWindow),
            ],
        },
    ]
}

pub struct IntervalApp {
    /// The replay store runtime: all session/playback/live state lives behind it, on
    /// the tokio runtime. Rendering reads it under a short-lived lock; user intents go
    /// through its methods.
    pub(crate) store: std::sync::Arc<interval_desktop_core::store::runtime::StoreRuntime>,
    /// The session selector's state machine (season/meeting/session + ingest flow).
    pub(crate) selector: std::sync::Arc<interval_desktop_core::selector::SelectorRuntime>,
    /// The monospace family everything data-bearing renders in. Resolved once at
    /// startup — the embedded JetBrains Mono normally, a system fallback otherwise.
    pub(crate) mono_font: gpui::SharedString,
    /// The titlebar drag latch: armed on mouse-down, consumed by the first move.
    pub(crate) titlebar_should_move: bool,
    /// Which dropdown is open, if any — at most one at a time.
    pub(crate) open_select: Option<views::ui::SelectKind>,
    /// Driver numbers hidden by the transport-bar filter: their run-timeline cards are
    /// dropped and their timing rows greyed. Session-scoped, never persisted.
    pub(crate) hidden_drivers: std::collections::HashSet<i32>,
    /// The seek bar's painted rectangle, probed each frame so pointer positions can be
    /// mapped to replay times.
    pub(crate) seek_bounds: std::rc::Rc<std::cell::Cell<Bounds<gpui::Pixels>>>,
    /// True while a seek-bar drag is in flight; the root's window-level mouse handlers
    /// keep it working when the pointer leaves the bar.
    pub(crate) scrubbing: bool,
    /// The timing tower's list scroll position, kept here so it survives re-renders.
    pub(crate) timing_scroll: gpui::UniformListScrollHandle,
    /// The track map's between-frames tween (the web frontend's rAF transition).
    pub(crate) map_animation: Option<views::MapAnimation>,
    /// The last snapshot the map rendered, kept to seed the next tween.
    pub(crate) map_last_snapshot: Option<interval_backend::domain::ReplaySnapshot>,
    /// The OpenF1 token popover's state.
    pub(crate) settings: views::SettingsUi,
    /// For the settings requests, which run outside the store runtimes.
    api: interval_desktop_core::api_client::ApiClient,
    /// Captured on the main thread, where main's runtime guard is active; requests
    /// must run on the tokio runtime (reqwest needs its reactor).
    tokio: tokio::runtime::Handle,
    focus_handle: gpui::FocusHandle,
}

impl IntervalApp {
    fn new(
        base_url: String,
        mono_font: gpui::SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        tracing::info!("IntervalApp::new");
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        let api = interval_desktop_core::api_client::ApiClient::new(base_url);
        let (repaint_tx, mut repaint_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let store = interval_desktop_core::store::runtime::StoreRuntime::start(
            api.clone(),
            persist::load_ui_state().last_session_key,
            Box::new(|key| {
                persist::save_ui_state(persist::UiState {
                    last_session_key: key,
                })
            }),
            repaint_tx.clone(),
        );
        let selector = interval_desktop_core::selector::SelectorRuntime::start(
            api.clone(),
            std::sync::Arc::clone(&store),
            repaint_tx,
        );

        // The store→UI bridge: one repaint per store change. tokio's mpsc futures
        // don't need the tokio reactor, so awaiting on GPUI's foreground executor is
        // fine; the loop ends when the entity drops.
        cx.spawn(async move |this, cx| {
            while repaint_rx.recv().await.is_some() {
                // Coalesce bursts: one notify per drained batch, so a chatty store
                // can never starve the platform event loop.
                while repaint_rx.try_recv().is_ok() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        // Save the window rectangle on close so the next launch reopens the same size
        // and place. All three `WindowBounds` variants carry the restore bounds, so a
        // window closed while maximized still remembers a sensible windowed size.
        let this = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            this.update(cx, |this: &mut IntervalApp, _cx| {
                this.persist_window_bounds(window)
            });
            true
        });

        let settings = views::SettingsUi::new(cx);
        let this = Self {
            store,
            selector,
            mono_font,
            titlebar_should_move: false,
            open_select: None,
            hidden_drivers: std::collections::HashSet::new(),
            seek_bounds: std::rc::Rc::default(),
            scrubbing: false,
            timing_scroll: gpui::UniformListScrollHandle::new(),
            map_animation: None,
            map_last_snapshot: None,
            settings,
            api,
            tokio: tokio::runtime::Handle::current(),
            focus_handle,
        };
        this.refresh_settings(cx);
        this
    }

    /// Fetches the token settings; any failure means "routes unavailable" (a web
    /// deployment) and the gear is simply never rendered — the frontend's behavior.
    fn refresh_settings(&self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tokio.spawn(async move {
            let _ = tx.send(api.openf1_token().await.ok());
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                this.settings.settings = Some(result);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn settings_save(&mut self, cx: &mut Context<Self>) {
        if self.settings.busy {
            return;
        }
        self.settings.busy = true;
        self.settings.error = None;
        self.settings.probe = None;
        let token = self.settings.token_input.trim().to_string();
        let api = self.api.clone();
        let store = std::sync::Arc::clone(&self.store);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tokio.spawn(async move {
            let outcome = async {
                api.save_openf1_token(&token).await?;
                let settings = api.openf1_token().await.ok();
                // A newly applied token can change live availability.
                store.check_live();
                let probe = api.test_openf1_token().await.ok();
                Ok::<_, interval_desktop_core::api_client::ApiError>((settings, probe))
            }
            .await;
            let _ = tx.send(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.settings.busy = false;
                match outcome {
                    Ok((settings, probe)) => {
                        this.settings.token_input.clear();
                        if settings.is_some() {
                            this.settings.settings = Some(settings);
                        }
                        this.settings.probe = probe;
                    }
                    Err(error) => {
                        this.settings.error =
                            Some(interval_desktop_core::settings_panel::save_error_message(
                                Some(&error.to_string()),
                            ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn settings_test(&mut self, cx: &mut Context<Self>) {
        if self.settings.busy {
            return;
        }
        self.settings.busy = true;
        self.settings.error = None;
        let api = self.api.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tokio.spawn(async move {
            let _ = tx.send(api.test_openf1_token().await);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.settings.busy = false;
                match outcome {
                    Ok(probe) => this.settings.probe = Some(probe),
                    Err(error) => {
                        this.settings.error =
                            Some(interval_desktop_core::settings_panel::save_error_message(
                                Some(&error.to_string()),
                            ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn settings_clear(&mut self, cx: &mut Context<Self>) {
        if self.settings.busy {
            return;
        }
        self.settings.busy = true;
        self.settings.error = None;
        self.settings.probe = None;
        let api = self.api.clone();
        let store = std::sync::Arc::clone(&self.store);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tokio.spawn(async move {
            let outcome = async {
                api.clear_openf1_token().await?;
                let settings = api.openf1_token().await.ok();
                store.check_live();
                Ok::<_, interval_desktop_core::api_client::ApiError>(settings)
            }
            .await;
            let _ = tx.send(outcome);
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.settings.busy = false;
                match outcome {
                    Ok(settings) => {
                        if settings.is_some() {
                            this.settings.settings = Some(settings);
                        }
                    }
                    Err(error) => {
                        this.settings.error =
                            Some(interval_desktop_core::settings_panel::save_error_message(
                                Some(&error.to_string()),
                            ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Maps a window x-coordinate onto the seek bar and seeks there.
    pub(crate) fn scrub_to(&mut self, x: gpui::Pixels, cx: &mut Context<Self>) {
        let bounds = self.seek_bounds.get();
        if bounds.size.width <= px(0.0) {
            return;
        }
        let fraction = ((x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
        let max_t = {
            let store = self.store.state();
            store.display_metadata().map(|meta| meta.max_t)
        };
        if let Some(max_t) = max_t {
            self.store.seek(fraction as f64 * max_t);
        }
        cx.notify();
    }

    /// Called from every way out: the platform's close request, and the `Quit` action
    /// the keybinding and the Linux caption button both dispatch.
    fn persist_window_bounds(&mut self, window: &Window) {
        let bounds = match window.window_bounds() {
            WindowBounds::Windowed(bounds)
            | WindowBounds::Maximized(bounds)
            | WindowBounds::Fullscreen(bounds) => bounds,
        };
        persist::save_window_state(persist::WindowState {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
        });
    }
}

/// The embedded JetBrains Mono when registration succeeded, otherwise the first
/// candidate the system actually has. gpui matches families by exact name against the
/// font database — there is no generic "monospace" alias and no substitution when the
/// name misses, so resolve once and cache.
fn resolve_mono_font(cx: &App) -> gpui::SharedString {
    const CANDIDATES: &[&str] = &[
        "JetBrains Mono",
        "Consolas",
        "Menlo",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Ubuntu Mono",
        "Source Code Pro",
        "Courier New",
    ];
    let available = cx.text_system().all_font_names();
    CANDIDATES
        .iter()
        .find(|candidate| available.iter().any(|name| name == *candidate))
        .map(|name| gpui::SharedString::from(*name))
        .or_else(|| {
            available
                .iter()
                .find(|name| name.to_ascii_lowercase().contains("mono"))
                .map(|name| gpui::SharedString::from(name.clone()))
        })
        .unwrap_or_else(|| gpui::SharedString::from("monospace"))
}

impl Render for IntervalApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            static FIRST_RENDER: std::sync::Once = std::sync::Once::new();
            FIRST_RENDER.call_once(|| tracing::info!("first render"));
        }
        // Resolved before the chain so style closures borrow this rather than `window`.
        let corners = views::client_corners(window);
        // Windows and macOS hide the native bar (`appears_transparent`) and always want
        // ours. Linux only gets it when the app actually owns the frame: the `Client`
        // decorations requested at open are downgraded to `Server` on an X11 session
        // with no compositor, and there the window manager draws a real titlebar that
        // ours would sit underneath.
        let owns_frame = !cfg!(target_os = "linux") || corners.is_some();
        let titlebar = owns_frame.then(|| views::titlebar(self, window, cx).into_any_element());

        // The dashboard shows once a session's metadata and a frame are in hand;
        // until then a centered status box explains what is happening (App.tsx).
        let dashboard_ready = {
            let store = self.store.state();
            store.display_metadata().is_some() && store.active_snapshot().is_some()
        };
        let load_message = (!dashboard_ready).then(|| {
            let selected_label = self.selector.state().selected_session_label();
            let store = self.store.state();
            interval_desktop_core::playback::replay_load_message(
                interval_desktop_core::playback::ReplayLoadMessageOptions {
                    metadata: store.display_metadata(),
                    metadata_loading: store.metadata.loading,
                    metadata_error: store.metadata.error.as_deref(),
                    snapshot_loading: store.snapshot_loading,
                    snapshot_error: store.snapshot_error.as_deref(),
                    session_key: store.session_key,
                    selected_session_label: selected_label.as_deref(),
                    live_status_message: store.live_availability_message.as_deref(),
                    live_connecting: store.live_transitioning(),
                },
            )
        });

        let selector_bar = views::session_selector(self, window, cx).into_any_element();
        let controls_bar = dashboard_ready
            .then(|| views::replay_controls(self, window, cx))
            .flatten()
            .map(|bar| bar.into_any_element());

        let root = div()
            .key_context("dashboard")
            .track_focus(&self.focus_handle)
            // Takes precedence over the app-level `Quit` handler registered in `main`,
            // which cannot see the window. Both the ctrl-q binding and the Linux
            // caption button's close arrive here, so every exit remembers the window
            // rectangle the way the platform's own close request already does.
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                this.persist_window_bounds(window);
                cx.quit();
            }))
            .on_action(cx.listener(|this, _: &TogglePlayback, _window, _cx| {
                this.store.toggle_playing();
            }))
            .on_action(cx.listener(|this, _: &SeekBackward, _window, _cx| {
                let t = this.store.state().time;
                this.store.seek(t - 15.0);
            }))
            .on_action(cx.listener(|this, _: &SeekForward, _window, _cx| {
                let t = this.store.state().time;
                this.store.seek(t + 15.0);
            }))
            // The macOS Window menu's two entries; handled here because both need the
            // window, which the app-level handlers in `main` cannot see.
            .on_action(cx.listener(|_, _: &MinimizeWindow, window, _cx| {
                window.minimize_window()
            }))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _cx| window.zoom_window()))
            // Scrubs track the pointer at the window level so dragging keeps working
            // when the pointer leaves the bar; release (or a move without the button
            // held) ends them.
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _window, cx| {
                if this.scrubbing {
                    if event.pressed_button == Some(gpui::MouseButton::Left) {
                        this.scrub_to(event.position.x, cx);
                    } else {
                        this.scrubbing = false;
                    }
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
                    if this.scrubbing {
                        this.scrubbing = false;
                        this.scrub_to(event.position.x, cx);
                    }
                }),
            )
            .flex()
            .flex_col()
            .size_full()
            .map(|el| views::round_client_corners(el, corners, views::ClientCorners::All))
            .bg(theme::CARBON())
            .text_color(theme::TEXT())
            .font_family(self.mono_font.clone())
            .children(titlebar)
            .child(selector_bar)
            .children(controls_bar)
            .child(match load_message {
                Some(message) => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_6()
                    .child(
                        div()
                            .border_1()
                            .border_color(theme::LINE())
                            .bg(theme::PANEL())
                            .px_4()
                            .py_3()
                            .text_sm()
                            .child(message),
                    )
                    .into_any_element(),
                // The dashboard grid: `minmax(34.5rem,38rem) | minmax(26rem,1fr) |
                // minmax(18rem,21rem)` over `1fr | 13rem`, approximated with flex
                // grow factors clamped by min/max widths. The timing minimum covers
                // the tower's 34.1rem of fixed columns plus its borders.
                None => {
                    let timing = views::timing_tower(self, window, cx).into_any_element();
                    let map = views::track_map(self, window, cx).into_any_element();
                    let side = views::side_panels(self, window, cx).into_any_element();
                    let stints = views::stint_timeline(self, window, cx).into_any_element();
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .gap_2()
                        .p_2()
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(gpui::rems(34.5))
                                        .max_w(gpui::rems(38.0))
                                        .min_h_0()
                                        .child(timing),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(gpui::rems(26.0))
                                        .min_h_0()
                                        .child(map),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(gpui::rems(18.0))
                                        .max_w(gpui::rems(21.0))
                                        .min_h_0()
                                        .child(side),
                                ),
                        )
                        .child(div().flex_none().h(gpui::rems(13.0)).child(stints))
                        .into_any_element()
                }
            });

        views::window_frame(root, window)
    }
}

fn main() {
    // Neither launch path has a stderr anyone reads — a Linux .desktop entry sets
    // Terminal=false and the Windows build is a windows_subsystem app — so a panic
    // before the window opens is indistinguishable from the icon doing nothing at
    // all. First thing in main, so it covers everything below; re-pointed into the
    // data dir's logs/ as soon as that directory exists.
    std::panic::set_hook(Box::new(|info| {
        let path = std::env::temp_dir().join("interval-desktop-panic.log");
        let backtrace = std::backtrace::Backtrace::force_capture();
        let _ = std::fs::write(&path, format!("{info}\n\n{backtrace}\n"));
        eprintln!("{info}");
    }));

    // Everything here happens before the tokio runtime exists: env mutation needs a
    // single-threaded process, and the chdir must precede any backend code (dotenv,
    // sqlite, curated tracks, FastF1 scripts are all cwd-relative).
    let data_dir = embed::data_dir().expect("failed to resolve data directory");
    embed::prepare_data_dir(&data_dir).expect("failed to prepare data directory");
    std::env::set_current_dir(&data_dir).expect("failed to enter data directory");
    let panic_log = data_dir.join("logs").join("desktop-panic.log");
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let _ = std::fs::write(&panic_log, format!("{info}\n\n{backtrace}\n"));
        eprintln!("{info}");
    }));
    embed::scrub_env();
    // After scrub (which strips INTERVAL_*) and before dotenv, so the embedded uv wins
    // over a stray `.env` entry and FastF1 ingest never needs a system Python.
    embed::provision_uv(&data_dir);
    interval_backend::env::load_dotenv();

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir.join("logs").join("desktop.log"))
        .expect("failed to open log file");
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "interval_backend=info,interval_desktop=info".to_string()),
        )
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    // The embedded backend lives on this runtime. It must outlive the UI, which
    // `run()` blocks for.
    let runtime = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    let _guard = runtime.enter();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let bound = runtime
        .block_on(embed::serve_embedded(async move {
            let _ = shutdown_rx.await;
        }))
        .expect("failed to start embedded backend");
    let base_url = format!("http://{}", bound.addr);
    tracing::info!(%base_url, "embedded backend ready");

    tracing::info!("entering gpui run loop");
    application().with_assets(assets::Assets).run(move |cx: &mut App| {
        tracing::info!("gpui app callback entered");
        cx.set_app_identity("com.interval.app", "interval");
        if let Err(error) = cx.text_system().add_fonts(
            assets::FONTS
                .iter()
                .map(|bytes| std::borrow::Cow::Borrowed(*bytes))
                .collect(),
        ) {
            tracing::warn!(%error, "failed to register embedded fonts; falling back to system");
        }
        let mono_font = resolve_mono_font(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // Transport keys are scoped so they don't fire while the settings token input
        // has focus (its node adds `settings_input` to the context stack).
        const TRANSPORT_KEYS: Option<&str> = Some("dashboard && !settings_input");
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("space", TogglePlayback, TRANSPORT_KEYS),
            KeyBinding::new("left", SeekBackward, TRANSPORT_KEYS),
            KeyBinding::new("right", SeekForward, TRANSPORT_KEYS),
        ]);
        if cfg!(target_os = "macos") {
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-h", Hide, None),
                KeyBinding::new("cmd-m", MinimizeWindow, None),
            ]);
            cx.on_action(|_: &Hide, cx| cx.hide());
            cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
            cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
            cx.set_menus(mac_menus());
        }
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // Restore the saved rectangle, but only if it still lands on a display — a
        // window saved on a monitor that is no longer attached would otherwise open
        // off-screen with no way to drag it back.
        let bounds = persist::load_window_state()
            .map(|saved| Bounds {
                origin: gpui::point(px(saved.x), px(saved.y)),
                size: size(px(saved.width), px(saved.height)),
            })
            .filter(|bounds| {
                cx.displays()
                    .iter()
                    .any(|display| display.bounds().intersects(bounds))
            })
            .unwrap_or_else(|| Bounds::centered(None, size(px(1440.0), px(900.0)), cx));
        tracing::info!("opening window");
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // The app draws its own themed titlebar (Phase 2) on every platform;
                // the native one is hidden on Windows/macOS.
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("interval".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
                }),
                // Asked for explicitly on Linux so the look is the same under every
                // window manager: leaving it to the platform gives a native bar on
                // compositors that implement xdg-decoration and none on the ones that
                // don't (GNOME/Mutter). Client means the app owns moving and resizing.
                window_decorations: cfg!(target_os = "linux")
                    .then_some(gpui::WindowDecorations::Client),
                // How a Linux desktop finds the window's identity: matched against a
                // .desktop file's basename (and StartupWMClass) for icon and app name.
                // The packaging phase's desktop entry must agree with this string.
                app_id: Some("interval".to_string()),
                // macOS: the app moves the window itself via start_window_move, which
                // also avoids AppKit's titlebar-click delay. No-op elsewhere.
                app_owns_titlebar_drag: true,
                window_min_size: Some(size(px(900.0), px(600.0))),
                ..Default::default()
            },
            |window, cx| {
                let base_url = base_url.clone();
                let mono_font = mono_font.clone();
                cx.new(|cx| IntervalApp::new(base_url, mono_font, window, cx))
            },
        )
        .expect("failed to open window");
        tracing::info!("window opened");
        cx.activate(true);
    });

    // `run()` returned: the last window closed and gpui quit. Drain the server so the
    // sqlite pool closes cleanly, then stop the runtime without waiting on stragglers.
    let _ = shutdown_tx.send(());
    let _ = runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(2), bound.task).await
    });
    drop(_guard);
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
}
