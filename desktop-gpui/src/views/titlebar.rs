//! The custom titlebar: themed like the rest of the window, draggable, with caption
//! buttons on Windows and Linux. macOS keeps its native traffic lights (the bar just
//! insets past them) and only needs the drag/double-click plumbing done in Rust;
//! Windows gets drag, double-click and snap layouts for free from the
//! `HTCAPTION`/`HTMAXBUTTON` hit-tests.
//!
//! Linux needs every one of those by hand. gpui's Linux backends leave
//! `on_hit_test_window_control` unimplemented, so `window_control_area` does nothing
//! there and the caption buttons have to drive the window themselves. Dragging uses the
//! same mouse-down latch as macOS. See `window_frame` for the resize edges.

use gpui::{Context, MouseButton, Window, div, prelude::*, px, svg};

use super::window_frame::{
    CLIENT_CORNER_RADIUS, ClientCorners, WIN_CORNER_RADIUS, client_corners, round_client_corners,
};
use crate::{IntervalApp, theme};

// Native caption metrics: Windows titlebars are a fixed 32px, macOS gets a touch more.
pub(crate) const TITLEBAR_HEIGHT: f32 = if cfg!(target_os = "windows") { 32.0 } else { 34.0 };
// Zed's measured inset for the macOS traffic lights (71px, +1px window border).
const TRAFFIC_LIGHT_PADDING: f32 = 71.0;

/// Windows and Linux draw their own caption buttons; macOS uses the native traffic lights.
const CAPTION_BUTTONS: bool = cfg!(any(target_os = "windows", target_os = "linux"));

pub fn titlebar(
    _app: &mut IntervalApp,
    window: &mut Window,
    cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let fullscreen = window.is_fullscreen();
    let maximized = window.is_maximized();
    let corners = client_corners(window);
    // The close button's hover fill reaches the window's top-right corner, so it rounds
    // itself too — DWM's radius on Windows 11, ours on a client-decorated Linux window.
    let close_radius = match corners {
        Some(tiling) => (!tiling.top && !tiling.right).then(|| px(CLIENT_CORNER_RADIUS)),
        None => (cfg!(target_os = "windows") && !maximized).then(|| px(WIN_CORNER_RADIUS)),
    };

    div()
        .id("titlebar")
        .window_control_area(gpui::WindowControlArea::Drag)
        .flex_none()
        .w_full()
        .h(px(TITLEBAR_HEIGHT))
        // The bar's own surface fill reaches the window's top corners, so it has to
        // round them itself — see `round_client_corners`.
        .map(|el| round_client_corners(el, corners, ClientCorners::Top))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .bg(theme::BAR_BG())
        .map(|el| {
            if fullscreen {
                el.pl_3()
            } else if cfg!(target_os = "macos") {
                el.pl(px(TRAFFIC_LIGHT_PADDING))
            } else {
                el.pl_3()
            }
        })
        // AppKit neither drags nor double-click-zooms a transparent titlebar for us
        // (`app_owns_titlebar_drag`), and Linux has no titlebar hit-testing at all, so
        // both do it by hand — the Zed latch pattern: arm on mouse-down, and the first
        // real move starts the native window drag.
        .when(cfg!(any(target_os = "macos", target_os = "linux")), |el| {
            el.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _window, _cx| this.titlebar_should_move = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _window, _cx| this.titlebar_should_move = false),
            )
            .on_mouse_move(cx.listener(|this, _, window, _cx| {
                if this.titlebar_should_move {
                    this.titlebar_should_move = false;
                    window.start_window_move();
                }
            }))
            .on_click(|event, window, _cx| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
        })
        // The window manager's own menu (Move, Resize, Always on Top, …), which a
        // server-side titlebar would have offered on right-click.
        .when(cfg!(target_os = "linux"), |el| {
            el.on_mouse_down(MouseButton::Right, |event, window, _cx| {
                window.show_window_menu(event.position);
            })
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    // The favicon's mint timing bars, reduced to a wordmark accent.
                    div()
                        .w(px(3.0))
                        .h(px(12.0))
                        .rounded(px(1.0))
                        .bg(theme::MINT()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::TEXT())
                        .child("interval"),
                ),
        )
        .when(CAPTION_BUTTONS && !fullscreen, |el| {
            el.child(
                div()
                    .flex()
                    .flex_row()
                    .h_full()
                    .child(caption_button(
                        "caption-min",
                        "icons/win-minimize.svg",
                        gpui::WindowControlArea::Min,
                        None,
                    ))
                    .child(caption_button(
                        "caption-max",
                        if maximized {
                            "icons/win-restore.svg"
                        } else {
                            "icons/win-maximize.svg"
                        },
                        gpui::WindowControlArea::Max,
                        None,
                    ))
                    .child(caption_button(
                        "caption-close",
                        "icons/win-close.svg",
                        gpui::WindowControlArea::Close,
                        close_radius,
                    )),
            )
        })
}

/// A caption button. On Windows there is no click handler: the `window_control_area`
/// tag routes the click through `WM_NCHITTEST`, and gpui + `DefWindowProc` do the
/// minimize/maximize/close. Linux ignores that tag, so there the button drives the
/// window directly. `occlude` is load-bearing on both — without it the surrounding
/// Drag hitbox wins the hit-test and the button is dead.
fn caption_button(
    id: &'static str,
    icon: &'static str,
    area: gpui::WindowControlArea,
    top_right_radius: Option<gpui::Pixels>,
) -> impl IntoElement {
    let close = matches!(area, gpui::WindowControlArea::Close);
    // The close button hovers Windows-red with a white glyph; the rest get a faint wash.
    let hover_bg: gpui::Hsla = if close {
        theme::CLOSE_RED()
    } else {
        theme::blend(theme::TEXT(), theme::BAR_BG(), 0.08)
    };
    div()
        .id(id)
        .group(id)
        .occlude()
        .window_control_area(area)
        .when(cfg!(target_os = "linux"), |el| {
            el.on_click(move |_event, window, cx| match area {
                gpui::WindowControlArea::Min => window.minimize_window(),
                gpui::WindowControlArea::Max => window.zoom_window(),
                // Not `remove_window`, which drops the window without running
                // `on_window_should_close` and would lose the saved window rectangle.
                gpui::WindowControlArea::Close => {
                    window.dispatch_action(Box::new(crate::Quit), cx)
                }
                gpui::WindowControlArea::Drag => {}
            })
        })
        .w(px(46.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .when_some(top_right_radius, |el, r| el.rounded_tr(r))
        .hover(|style| style.bg(hover_bg))
        .active(|style| style.bg(hover_bg))
        .child(
            svg()
                .path(icon)
                .size(px(10.0))
                .text_color(theme::TEXT())
                .when(close, |el| {
                    el.group_hover(id, |style| style.text_color(gpui::white()))
                }),
        )
}
