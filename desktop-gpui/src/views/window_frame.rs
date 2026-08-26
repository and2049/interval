//! The compositor-substitute window frame for client-decorated Linux windows.
//! Ported from echo (`crates/echo-desktop/src/views.rs`), where every constant and
//! branch is the product of a shipped bug — see that repo's gpui-cross-platform notes.

use gpui::{AnyElement, MouseButton, Window, div, prelude::*, px};

use crate::theme;

/// Corner radius and drop-shadow depth for a client-decorated window, matching what
/// GNOME and Zed use so interval sits alongside them without looking off.
pub(crate) const CLIENT_CORNER_RADIUS: f32 = 10.0;
/// The radius DWM clips an unmaximized window to on Windows 11.
pub(crate) const WIN_CORNER_RADIUS: f32 = 8.0;
const CLIENT_SHADOW: f32 = 10.0;
/// Grab bands, sized to the transparent shadow margin so they sit beside the visible
/// window rather than over its content. The corners reach a little further in.
const RESIZE_BAND: f32 = CLIENT_SHADOW;
const RESIZE_CORNER: f32 = CLIENT_SHADOW * 2.0;

/// Which corners of an element sit against the window's own, for [`round_client_corners`].
#[derive(Clone, Copy)]
pub enum ClientCorners {
    All,
    Top,
}

/// The window's tiling state when the app is drawing its own frame, `None` when the
/// compositor draws it and the corners are not ours to round. Resolve this once per
/// render and hand it around — the alternative is borrowing the window inside a style
/// closure.
pub fn client_corners(window: &Window) -> Option<gpui::Tiling> {
    match window.window_decorations() {
        gpui::Decorations::Client { tiling } => Some(tiling),
        gpui::Decorations::Server => None,
    }
}

/// Rounds the corners an element shares with the window's.
///
/// gpui's content mask is a plain rectangle, so rounding a container does **not** clip
/// what is inside it: any child that paints an opaque background into a corner squares
/// it off again. That makes this a per-element job rather than one wrapper — the root's
/// background, the titlebar's, and the close button's hover fill are the surfaces that
/// reach a corner. An edge that is tiled is flush against a screen or a neighbour, and
/// stays square, as every other app's does.
pub fn round_client_corners<E: Styled>(
    mut el: E,
    corners: Option<gpui::Tiling>,
    which: ClientCorners,
) -> E {
    let Some(tiling) = corners else { return el };
    let radius = px(CLIENT_CORNER_RADIUS);

    if !tiling.top && !tiling.left {
        el = el.rounded_tl(radius);
    }
    if !tiling.top && !tiling.right {
        el = el.rounded_tr(radius);
    }
    if matches!(which, ClientCorners::All) {
        if !tiling.bottom && !tiling.left {
            el = el.rounded_bl(radius);
        }
        if !tiling.bottom && !tiling.right {
            el = el.rounded_br(radius);
        }
    }
    el
}

/// Wraps the whole app in the frame a window manager would normally provide.
///
/// A compositor that does not implement xdg-decoration — GNOME/Mutter, notably — hands
/// the window back as `Decorations::Client` and draws nothing itself: no titlebar, no
/// border, no shadow, and crucially no resize handles. The titlebar covers moving and
/// the caption buttons; this covers the frame, as a transparent margin around the app
/// carrying the drop shadow, with the resize strips laid into that margin so they grab
/// beside the window rather than over its content.
///
/// A pass-through everywhere else: Windows, macOS, and any Linux WM that draws its own
/// decorations all report `Decorations::Server`, and then the real frame already works.
pub fn window_frame(root: impl IntoElement, window: &mut Window) -> AnyElement {
    let Some(tiling) = client_corners(window) else {
        return root.into_any_element();
    };
    let shadow = px(CLIENT_SHADOW);
    // Tells the compositor the visible window is inset from the surface by the shadow
    // margin, so snapping and edge detection use the frame the user sees rather than
    // the transparent one.
    window.set_client_inset(shadow);
    let resizable = window.is_resizable();

    // Edges are laid down first and corners on top, so the corners win where they overlap.
    let edges: [(&'static str, gpui::ResizeEdge, bool); 8] = [
        ("resize-top", gpui::ResizeEdge::Top, tiling.top),
        ("resize-bottom", gpui::ResizeEdge::Bottom, tiling.bottom),
        ("resize-left", gpui::ResizeEdge::Left, tiling.left),
        ("resize-right", gpui::ResizeEdge::Right, tiling.right),
        (
            "resize-top-left",
            gpui::ResizeEdge::TopLeft,
            tiling.top || tiling.left,
        ),
        (
            "resize-top-right",
            gpui::ResizeEdge::TopRight,
            tiling.top || tiling.right,
        ),
        (
            "resize-bottom-left",
            gpui::ResizeEdge::BottomLeft,
            tiling.bottom || tiling.left,
        ),
        (
            "resize-bottom-right",
            gpui::ResizeEdge::BottomRight,
            tiling.bottom || tiling.right,
        ),
    ];

    div()
        .relative()
        .size_full()
        // The transparent margin: room for the shadow to fall into, and where the grab
        // strips live. A tiled edge has neither, so the window still meets the screen
        // edge exactly.
        .when(!tiling.top, |el| el.pt(shadow))
        .when(!tiling.bottom, |el| el.pb(shadow))
        .when(!tiling.left, |el| el.pl(shadow))
        .when(!tiling.right, |el| el.pr(shadow))
        .child(
            div()
                .size_full()
                .map(|el| round_client_corners(el, Some(tiling), ClientCorners::All))
                // A hairline outline stands in for the frame the compositor is not
                // drawing, so the window still reads as one against a same-coloured
                // background behind it.
                .border_1()
                .border_color(theme::LINE())
                .when(!tiling.is_tiled(), |el| {
                    el.shadow(vec![gpui::BoxShadow::new(
                        px(0.0),
                        px(2.0),
                        gpui::hsla(0.0, 0.0, 0.0, 0.36),
                    )
                    .blur_radius(shadow / 2.0)])
                })
                .child(root),
        )
        .children(edges.into_iter().filter_map(|(id, edge, tiled)| {
            // A tiled edge is flush against a screen or neighbour and cannot be dragged.
            (resizable && !tiled).then(|| resize_handle(id, edge))
        }))
        .into_any_element()
}

/// One transparent grab strip, positioned against the outer edge of the shadow margin.
/// `occlude` keeps it above the app's own hitboxes, which matters at the corners, where
/// the square reaches past the margin into the window.
fn resize_handle(id: &'static str, edge: gpui::ResizeEdge) -> impl IntoElement {
    use gpui::ResizeEdge as E;
    let band = px(RESIZE_BAND);
    let corner = px(RESIZE_CORNER);

    div()
        .id(id)
        .occlude()
        .absolute()
        .cursor(match edge {
            E::Top | E::Bottom => gpui::CursorStyle::ResizeUpDown,
            E::Left | E::Right => gpui::CursorStyle::ResizeLeftRight,
            E::TopLeft | E::BottomRight => gpui::CursorStyle::ResizeUpLeftDownRight,
            E::TopRight | E::BottomLeft => gpui::CursorStyle::ResizeUpRightDownLeft,
        })
        .map(|el| match edge {
            E::Top => el.top_0().left_0().w_full().h(band),
            E::Bottom => el.bottom_0().left_0().w_full().h(band),
            E::Left => el.top_0().left_0().h_full().w(band),
            E::Right => el.top_0().right_0().h_full().w(band),
            E::TopLeft => el.top_0().left_0().w(corner).h(corner),
            E::TopRight => el.top_0().right_0().w(corner).h(corner),
            E::BottomLeft => el.bottom_0().left_0().w(corner).h(corner),
            E::BottomRight => el.bottom_0().right_0().w(corner).h(corner),
        })
        .on_mouse_down(MouseButton::Left, move |_event, window, _cx| {
            window.start_window_resize(edge);
        })
}
