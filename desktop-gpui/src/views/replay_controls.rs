//! The transport bar: session title, race clock, LIVE/SIM toggles, restart/±15s/
//! play-pause, channel-health badges, the seek bar and speed select — the port of
//! `frontend/src/components/ReplayControls.tsx`.

use gpui::{Context, MouseButton, SharedString, Window, canvas, div, prelude::*, px, rems, svg};
use interval_desktop_core::store::{LiveConnection, SimConnection};
use interval_desktop_core::{formatters, playback, replay_quality};

use super::ui::{self, SelectKind, SelectOption};
use crate::{IntervalApp, theme};

struct ControlsSnapshot {
    title: String,
    t: f64,
    max_t: f64,
    playing: bool,
    speed: f64,
    live_active: bool,
    live_checking: bool,
    sim_active: bool,
    status_label: String,
    badges: Vec<replay_quality::ChannelBadge>,
}

fn snapshot(app: &IntervalApp) -> Option<ControlsSnapshot> {
    let store = app.store.state();
    let metadata = store.display_metadata()?;
    let live_connection_label = match store.live_connection {
        LiveConnection::Idle => "idle",
        LiveConnection::Connecting => "connecting",
        LiveConnection::Connected => "connected",
        LiveConnection::Reconnecting => "reconnecting",
        LiveConnection::Disconnected => "disconnected",
    };
    let status_label = if store.live_active {
        replay_quality::live_status_label(
            Some(live_connection_label),
            store.live_status.as_ref(),
            chrono::Utc::now().timestamp_millis(),
        )
    } else if store.live_simulation_active {
        let sim = match store.live_simulation_connection {
            SimConnection::Idle => "idle",
            SimConnection::Connecting => "connecting",
            SimConnection::Connected => "connected",
            SimConnection::Disconnected => "disconnected",
        };
        format!("SIM {sim}")
    } else {
        "REPLAY".to_string()
    };
    let badges = if store.live_active {
        replay_quality::live_dashboard_badges(metadata, store.live_channels())
    } else {
        replay_quality::channel_badges(metadata)
    };
    Some(ControlsSnapshot {
        title: playback::replay_session_title(metadata),
        t: store.time,
        max_t: metadata.max_t,
        playing: store.playing,
        speed: store.speed,
        live_active: store.live_active,
        live_checking: store.live_availability_checking,
        sim_active: store.live_simulation_active,
        status_label,
        badges,
    })
}

pub fn replay_controls(
    app: &mut IntervalApp,
    _window: &mut Window,
    cx: &mut Context<IntervalApp>,
) -> Option<impl IntoElement> {
    let state = snapshot(app)?;
    let controls_locked = state.live_active || state.sim_active;
    let max_t = state.max_t;
    let t = state.t;

    let icon_button = |id: &'static str,
                       icon: &'static str,
                       icon_px: f32,
                       accent: bool,
                       disabled: bool,
                       cx: &mut Context<IntervalApp>,
                       on_click: fn(&mut IntervalApp)| {
        div()
            .id(id)
            .rounded(px(3.0))
            .border_1()
            .p_2()
            .map(|el| {
                if disabled {
                    el.border_color(theme::LINE())
                } else if accent {
                    el.border_color(theme::MINT())
                        .bg(theme::blend(theme::MINT(), theme::BAR_BG(), 0.1))
                } else {
                    el.border_color(theme::LINE())
                        .cursor_pointer()
                        .hover(|style| style.border_color(theme::MINT()))
                }
            })
            .when(!disabled, |el| {
                el.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _window, _cx| on_click(this)))
            })
            .child(svg().path(icon).size(px(icon_px)).text_color(if disabled {
                ui::faint()
            } else if accent {
                theme::MINT()
            } else {
                theme::TEXT()
            }))
    };

    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .border_b_1()
            .border_color(theme::LINE())
            .bg(theme::BAR_BG())
            .px_3()
            .py_2()
            .font_family(app.mono_font.clone())
            // Left: session title + clock.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(rems(0.68))
                                    .text_color(ui::muted())
                                    .child("SESSION"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(state.title),
                            ),
                    )
                    .child(div().h_8().border_l_1().border_color(theme::LINE()))
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(rems(0.68))
                                    .text_color(ui::muted())
                                    .child("CLOCK"),
                            )
                            .child(
                                div()
                                    .text_xl()
                                    .text_color(gpui::white())
                                    .child(formatters::format_race_clock(state.t)),
                            ),
                    ),
            )
            // Center: LIVE / SIM / transport.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("live-toggle")
                            .rounded(px(3.0))
                            .border_1()
                            .px_2()
                            .py_2()
                            .text_size(rems(0.65))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .map(|el| {
                                let disabled = state.live_checking || state.sim_active;
                                if disabled {
                                    el.border_color(theme::LINE()).text_color(ui::faint())
                                } else if state.live_active {
                                    el.border_color(theme::DANGER())
                                        .bg(theme::blend(
                                            theme::DANGER(),
                                            theme::BAR_BG(),
                                            0.1,
                                        ))
                                        .text_color(theme::DANGER())
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _window, _cx| {
                                            this.store.stop_live();
                                        }))
                                } else {
                                    el.border_color(theme::MINT())
                                        .bg(theme::blend(theme::MINT(), theme::BAR_BG(), 0.1))
                                        .text_color(theme::MINT())
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _window, _cx| {
                                            this.store.check_live();
                                        }))
                                }
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        svg()
                                            .path("icons/radio.svg")
                                            .size(px(13.0))
                                            .text_color(if state.live_active {
                                                theme::DANGER()
                                            } else {
                                                theme::MINT()
                                            }),
                                    )
                                    .child(if state.live_active {
                                        "STOP LIVE"
                                    } else if state.live_checking {
                                        "CHECKING"
                                    } else {
                                        "OPEN LIVE"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("live-sim-toggle")
                            .rounded(px(3.0))
                            .border_1()
                            .px_2()
                            .py_2()
                            .text_size(rems(0.65))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .map(|el| {
                                if state.live_active {
                                    el.border_color(theme::LINE()).text_color(ui::faint())
                                } else if state.sim_active {
                                    el.border_color(theme::DANGER())
                                        .bg(theme::blend(
                                            theme::DANGER(),
                                            theme::BAR_BG(),
                                            0.1,
                                        ))
                                        .text_color(theme::DANGER())
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _window, _cx| {
                                            this.store.toggle_live_simulation();
                                        }))
                                } else {
                                    el.border_color(theme::LINE())
                                        .bg(theme::PANEL())
                                        .text_color(ui::muted())
                                        .cursor_pointer()
                                        .hover(|style| {
                                            style
                                                .border_color(theme::MINT())
                                                .text_color(theme::MINT())
                                        })
                                        .on_click(cx.listener(|this, _, _window, _cx| {
                                            this.store.toggle_live_simulation();
                                        }))
                                }
                            })
                            .child(if state.sim_active { "STOP SIM" } else { "SIM" }),
                    )
                    .child(icon_button(
                        "replay-restart",
                        "icons/rotate-ccw.svg",
                        16.0,
                        false,
                        controls_locked,
                        cx,
                        |this| this.store.seek(0.0),
                    ))
                    .child(icon_button(
                        "replay-back",
                        "icons/step-back.svg",
                        16.0,
                        false,
                        controls_locked,
                        cx,
                        |this| {
                            let t = this.store.state().time;
                            this.store.seek(t - 15.0);
                        },
                    ))
                    .child(icon_button(
                        "replay-play-toggle",
                        if state.playing {
                            "icons/pause.svg"
                        } else {
                            "icons/play.svg"
                        },
                        18.0,
                        true,
                        controls_locked,
                        cx,
                        |this| this.store.toggle_playing(),
                    ))
                    .child(icon_button(
                        "replay-forward",
                        "icons/step-forward.svg",
                        16.0,
                        false,
                        controls_locked,
                        cx,
                        |this| {
                            let t = this.store.state().time;
                            this.store.seek(t + 15.0);
                        },
                    )),
            )
            // Right: badges, seek, speed, mode label.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap_3()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .children(state.badges.iter().map(ui::channel_badge)),
                    )
                    .child(seek_bar(t, max_t, controls_locked, app, cx))
                    .child(ui::select_field(
                        "replay-speed",
                        SelectKind::Speed,
                        "",
                        SharedString::from(format_speed(state.speed)),
                        vec![
                            speed_option(0.5),
                            speed_option(1.0),
                            speed_option(2.0),
                            speed_option(4.0),
                        ],
                        Some(speed_key(state.speed)),
                        controls_locked,
                        |this, key, _window, _cx| this.store.set_speed(key as f64 / 2.0),
                        app,
                        cx,
                    ))
                    .child(
                        div()
                            .border_1()
                            .border_color(theme::LINE())
                            .px(px(6.0))
                            .py(px(2.0))
                            .text_size(rems(0.62))
                            .text_color(ui::muted())
                            .whitespace_nowrap()
                            .child(state.status_label.to_uppercase()),
                    ),
            ),
    )
}

/// Speeds are keyed as `speed * 2` so the select's value type stays integral.
fn speed_key(speed: f64) -> i64 {
    (speed * 2.0).round() as i64
}

fn format_speed(speed: f64) -> String {
    if speed.fract() == 0.0 {
        format!("{}x", speed as i64)
    } else {
        format!("{speed}x")
    }
}

fn speed_option(speed: f64) -> SelectOption<i64> {
    SelectOption {
        value: speed_key(speed),
        label: SharedString::from(format_speed(speed)),
        disabled: false,
    }
}

/// The seek slider: a track with a fill and handle. Dragging is tracked at the window
/// level (the root's mouse handlers call [`IntervalApp::update_scrub`]) so it keeps
/// working when the pointer leaves the bar.
fn seek_bar(
    t: f64,
    max_t: f64,
    disabled: bool,
    app: &IntervalApp,
    cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let fraction = if max_t > 0.0 {
        (t / max_t).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let bounds_probe = app.seek_bounds.clone();
    div()
        .id("replay-seek")
        .relative()
        .w(px(220.0))
        .h(px(16.0))
        .flex()
        .items_center()
        .when(!disabled, |el| {
            el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.scrubbing = true;
                    this.scrub_to(event.position.x, cx);
                }),
            )
        })
        .child(
            canvas(
                move |bounds, _window, _cx| bounds_probe.set(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .w_full()
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(theme::LINE())
                .child(
                    div()
                        .w(gpui::relative(fraction))
                        .h_full()
                        .rounded(px(2.0))
                        .bg(if disabled { ui::faint() } else { theme::MINT() }),
                ),
        )
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(gpui::relative(fraction))
                .ml(px(-5.0))
                .w(px(10.0))
                .h(px(10.0))
                .rounded_full()
                .bg(if disabled { ui::faint() } else { theme::MINT() }),
        )
}
