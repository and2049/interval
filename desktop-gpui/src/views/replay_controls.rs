//! The transport bar: session title, race clock, LIVE/SIM toggles, restart/±15s/
//! play-pause, the seek bar and speed select — the port of
//! `frontend/src/components/ReplayControls.tsx` (minus its channel-health badge strip).

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
    sim_active: bool,
    status_label: String,
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
    Some(ControlsSnapshot {
        title: playback::replay_session_title(metadata),
        t: store.time,
        max_t: metadata.max_t,
        playing: store.playing,
        speed: store.speed,
        live_active: store.live_active,
        sim_active: store.live_simulation_active,
        status_label,
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
            .p(px(5.0))
            .map(|el| {
                if disabled {
                    el.border_color(theme::LINE())
                } else if accent {
                    el.border_color(theme::ACCENT())
                        .bg(theme::blend(theme::ACCENT(), theme::BAR_BG(), 0.1))
                } else {
                    el.border_color(theme::LINE())
                        .cursor_pointer()
                        .hover(|style| style.border_color(theme::ACCENT()))
                }
            })
            .when(!disabled, |el| {
                el.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _window, _cx| on_click(this)))
            })
            .child(svg().path(icon).size(px(icon_px)).text_color(if disabled {
                ui::faint()
            } else if accent {
                theme::ACCENT()
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
            .py(px(6.0))
            .font_family(app.mono_font.clone())
            // Left: session title · clock, one quiet line.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .min_w_0()
                            .text_size(rems(0.72))
                            .text_color(ui::muted())
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(state.title),
                    )
                    .child(
                        div()
                            .h_4()
                            .flex_shrink_0()
                            .border_l_1()
                            .border_color(theme::LINE()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(rems(0.78))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child(formatters::format_race_clock(state.t)),
                    ),
            )
            // Center: LIVE / SIM / transport. `flex_none` mirrors the web grid's `auto`
            // middle column — the buttons never compress into their neighbors.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_none()
                    .gap_1()
                    // Opening live lives in the session bar; this button only appears
                    // once a live session owns the dashboard, as the way out.
                    .when(state.live_active, |el| {
                        el.child(
                            div()
                                .id("live-toggle")
                                .rounded(px(3.0))
                                .border_1()
                                .px_2()
                                .py(px(4.0))
                                .text_size(rems(0.65))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .border_color(theme::DANGER())
                                .bg(theme::blend(theme::DANGER(), theme::BAR_BG(), 0.1))
                                .text_color(theme::DANGER())
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _window, _cx| {
                                    this.store.stop_live();
                                }))
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
                                                .text_color(theme::DANGER()),
                                        )
                                        .child("STOP LIVE"),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("live-sim-toggle")
                            .rounded(px(3.0))
                            .border_1()
                            .px_2()
                            .py(px(4.0))
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
                                        .text_color(ui::muted())
                                        .cursor_pointer()
                                        .hover(|style| {
                                            style
                                                .border_color(theme::ACCENT())
                                                .text_color(theme::ACCENT())
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
                        14.0,
                        false,
                        controls_locked,
                        cx,
                        |this| this.store.seek(0.0),
                    ))
                    .child(icon_button(
                        "replay-back",
                        "icons/step-back.svg",
                        14.0,
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
                        16.0,
                        true,
                        controls_locked,
                        cx,
                        |this| this.store.toggle_playing(),
                    ))
                    .child(icon_button(
                        "replay-forward",
                        "icons/step-forward.svg",
                        14.0,
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
                    .children(driver_filter(app, cx))
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
                            .flex_shrink_0()
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

/// The driver filter: a dropdown of every driver in the current frame. Toggling a row
/// hides that driver's run-timeline card and greys their timing row; the menu stays
/// open across toggles. State lives on [`IntervalApp::hidden_drivers`] only — nothing
/// persists, and it works the same for replays and live sessions.
fn driver_filter(
    app: &IntervalApp,
    cx: &mut Context<IntervalApp>,
) -> Option<impl IntoElement + use<>> {
    let mut drivers: Vec<(i32, String, gpui::Hsla)> = {
        let store = app.store.state();
        store
            .active_snapshot()?
            .timing
            .rows
            .iter()
            .map(|row| {
                (
                    row.driver.driver_number,
                    row.driver.code.clone(),
                    theme::team_colour(&row.driver.team_colour),
                )
            })
            .collect()
    };
    if drivers.is_empty() {
        return None;
    }
    // Alphabetical, so rows don't jump around as positions change mid-session.
    drivers.sort_by(|a, b| a.1.cmp(&b.1));

    let hidden_count = app.hidden_drivers.len();
    let open = app.open_select == Some(SelectKind::DriverFilter);
    let filtering = hidden_count > 0;
    let label = if filtering {
        format!("FILTER −{hidden_count} ▾")
    } else {
        "FILTER ▾".to_string()
    };

    Some(
        div()
            .id("driver-filter")
            .relative()
            .flex_shrink_0()
            .border_1()
            .border_color(if filtering {
                theme::ACCENT()
            } else {
                theme::LINE()
            })
            .bg(theme::PANEL())
            .px_2()
            .py(px(3.0))
            .text_size(rems(0.72))
            .text_color(if filtering { theme::TEXT() } else { ui::muted() })
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(|style| style.border_color(theme::ACCENT()))
            .on_click(cx.listener(|this, _, _window, cx| {
                this.open_select = if this.open_select == Some(SelectKind::DriverFilter) {
                    None
                } else {
                    Some(SelectKind::DriverFilter)
                };
                cx.notify();
            }))
            .child(label)
            .when(open, |el| {
                el.child(gpui::deferred(
                    gpui::anchored().snap_to_window_with_margin(px(8.0)).child(
                        div()
                            .id("driver-filter-menu")
                            .occlude()
                            .mt(px(2.0))
                            .min_w(px(150.0))
                            .max_h(px(360.0))
                            .overflow_y_scroll()
                            .border_1()
                            .border_color(theme::LINE())
                            .bg(theme::PANEL())
                            .shadow_md()
                            .text_size(rems(0.72))
                            .on_mouse_down_out(cx.listener(|this, _, _window, cx| {
                                this.open_select = None;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .id("driver-filter-all")
                                    .px_2()
                                    .py(px(4.0))
                                    .border_b_1()
                                    .border_color(theme::LINE())
                                    .text_color(if filtering {
                                        theme::TEXT()
                                    } else {
                                        ui::faint()
                                    })
                                    .when(filtering, |el| {
                                        el.cursor_pointer()
                                            .hover(|style| style.bg(theme::PANEL_HI()))
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.hidden_drivers.clear();
                                                cx.notify();
                                            }))
                                    })
                                    .child("SHOW ALL"),
                            )
                            .children(drivers.into_iter().enumerate().map(
                                |(index, (number, code, colour))| {
                                    let visible = !app.hidden_drivers.contains(&number);
                                    div()
                                        .id(index)
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .px_2()
                                        .py(px(4.0))
                                        .cursor_pointer()
                                        .hover(|style| style.bg(theme::PANEL_HI()))
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            if !this.hidden_drivers.remove(&number) {
                                                this.hidden_drivers.insert(number);
                                            }
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex_none()
                                                .w(px(8.0))
                                                .h(px(8.0))
                                                .bg(if visible {
                                                    colour
                                                } else {
                                                    theme::blend(colour, theme::PANEL(), 0.3)
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_color(if visible {
                                                    theme::TEXT()
                                                } else {
                                                    ui::faint()
                                                })
                                                .child(code),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(theme::ACCENT())
                                                .child(if visible { "✓" } else { " " }),
                                        )
                                },
                            )),
                    ),
                ))
            }),
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
        .flex_shrink_0()
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
                        .bg(if disabled { ui::faint() } else { theme::ACCENT() }),
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
                .bg(if disabled { ui::faint() } else { theme::ACCENT() }),
        )
}
