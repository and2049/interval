//! Small shared widgets for the dashboard bars and panels: tone→color mapping,
//! bordered badges, and the dropdown select the two top bars use in place of the web
//! frontend's `<select>`s.

use gpui::{Context, Hsla, SharedString, Window, div, prelude::*, px, rems};
use interval_backend::domain::SectorStatus;
use interval_desktop_core::formatters::Tone;
use interval_desktop_core::replay_quality::{BadgeTone, ChannelBadge};
use interval_desktop_core::session_readiness;

use crate::{IntervalApp, theme};

// The F1 broadcast pace colors: purple = overall best, green = personal best,
// yellow = no improvement.
pub const PACE_PURPLE: fn() -> Hsla = || gpui::rgb(0xc084fc).into();
pub const PACE_GREEN: fn() -> Hsla = || gpui::rgb(0x34d399).into();
pub const PACE_YELLOW: fn() -> Hsla = || gpui::rgb(0xf3d24f).into();

/// Color for a sector/lap time by its pace status. `Unknown` with a real time only
/// happens on snapshots cached before statuses were computed — render those plain.
pub fn pace_color(status: &SectorStatus) -> Hsla {
    match status {
        SectorStatus::OverallBest => PACE_PURPLE(),
        SectorStatus::PersonalBest => PACE_GREEN(),
        SectorStatus::Normal => PACE_YELLOW(),
        SectorStatus::Unknown => theme::TIMING(),
    }
}

/// The one `formatters::Tone` → color mapping, shared by every view. Chromatic hexes
/// are functional only (severity fuchsia, inter/wet tyre green/blue); everything else
/// resolves to a theme token so the greyscale stays consistent.
pub fn tone_color(tone: Tone) -> Hsla {
    match tone {
        Tone::Fuchsia => gpui::rgb(0xe879f9).into(),
        Tone::Mint => theme::ACCENT(),
        Tone::Timing => theme::TIMING(),
        Tone::Danger => theme::DANGER(),
        Tone::Amber => theme::AMBER(),
        Tone::Bright => theme::TEXT(),
        Tone::Emerald => gpui::rgb(0x34d399).into(),
        Tone::Sky => gpui::rgb(0x38bdf8).into(),
        Tone::Neutral => theme::blend(theme::TEXT(), theme::CARBON(), 0.78),
        Tone::Muted => muted(),
    }
}

/// Muted slate the frontend used for secondary text (`text-slate-400/500`).
pub fn muted() -> Hsla {
    theme::blend(theme::TEXT(), theme::CARBON(), 0.55)
}

pub fn faint() -> Hsla {
    theme::blend(theme::TEXT(), theme::CARBON(), 0.38)
}

/// Border+text color for a channel/status badge tone.
pub fn badge_tone_color(tone: BadgeTone) -> Hsla {
    match tone {
        BadgeTone::Ready => theme::ACCENT(),
        BadgeTone::Degraded => theme::AMBER(),
        BadgeTone::Missing => muted(),
    }
}

pub fn quality_tone_color(tone: interval_desktop_core::replay_quality::Tone) -> Hsla {
    use interval_desktop_core::replay_quality::Tone;
    match tone {
        Tone::Mint => theme::ACCENT(),
        Tone::Amber => theme::AMBER(),
        Tone::Neutral => muted(),
    }
}

pub fn readiness_tone_color(tone: session_readiness::Tone) -> Hsla {
    match tone {
        session_readiness::Tone::Mint => theme::ACCENT(),
        session_readiness::Tone::Amber => theme::AMBER(),
        session_readiness::Tone::Danger => theme::DANGER(),
        session_readiness::Tone::Neutral => muted(),
    }
}

/// A bordered uppercase mono badge (`border px-2 py-1` in the web frontend).
pub fn badge(label: impl Into<SharedString>, color: Hsla) -> gpui::Div {
    div()
        .border_1()
        .border_color(theme::blend(color, theme::CARBON(), 0.4))
        .text_color(color)
        .px(px(8.0))
        .py(px(2.0))
        .text_size(rems(0.68))
        .whitespace_nowrap()
        .child(label.into())
}

pub fn channel_badge(entry: &ChannelBadge) -> gpui::Div {
    badge(
        entry.label.to_uppercase(),
        badge_tone_color(entry.tone),
    )
}

/// One option row for [`select_field`].
pub struct SelectOption<V> {
    pub value: V,
    pub label: SharedString,
    pub disabled: bool,
}

/// Which dropdown is currently open; lives on [`IntervalApp`] so at most one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    Season,
    Meeting,
    Session,
    Speed,
    DriverFilter,
}

/// A labelled dropdown: a bordered button showing the current value, and when open an
/// anchored panel of options. `on_pick` receives the chosen value; picking or clicking
/// elsewhere closes it.
pub fn select_field<V: Copy + PartialEq + 'static>(
    id: &'static str,
    kind: SelectKind,
    label: &'static str,
    value_label: SharedString,
    options: Vec<SelectOption<V>>,
    selected: Option<V>,
    disabled: bool,
    on_pick: impl Fn(&mut IntervalApp, V, &mut Window, &mut Context<IntervalApp>) + 'static,
    app: &IntervalApp,
    cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let open = app.open_select == Some(kind);
    let on_pick = std::rc::Rc::new(on_pick);
    div()
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .gap_1()
        .child(
            div()
                .text_color(faint())
                .text_size(rems(0.72))
                .child(label),
        )
        .child(
            div()
                .id(id)
                .relative()
                .border_1()
                .border_color(theme::LINE())
                .bg(theme::PANEL())
                .px_2()
                .py(px(3.0))
                .text_size(rems(0.72))
                .text_color(if disabled { faint() } else { theme::TEXT() })
                .when(!disabled, |el| {
                    el.cursor_pointer()
                        .hover(|style| style.border_color(theme::ACCENT()))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.open_select = if this.open_select == Some(kind) {
                                None
                            } else {
                                Some(kind)
                            };
                            cx.notify();
                        }))
                })
                .child(format!("{value_label} ▾"))
                .when(open, |el| {
                    el.child(gpui::deferred(
                        gpui::anchored().snap_to_window_with_margin(px(8.0)).child(
                            div()
                                .id(SharedString::from(format!("{id}-menu")))
                                .occlude()
                                .mt(px(2.0))
                                .min_w(px(160.0))
                                .max_w(px(420.0))
                                .max_h(px(360.0))
                                .overflow_y_scroll()
                                .border_1()
                                .border_color(theme::LINE())
                                .bg(theme::PANEL())
                                .shadow_md()
                                .on_mouse_down_out(cx.listener(|this, _, _window, cx| {
                                    this.open_select = None;
                                    cx.notify();
                                }))
                                .children(options.into_iter().enumerate().map(
                                    |(index, option)| {
                                        let on_pick = on_pick.clone();
                                        let is_selected = selected == Some(option.value);
                                        div()
                                            .id(index)
                                            .px_2()
                                            .py(px(4.0))
                                            .text_size(rems(0.72))
                                            .whitespace_nowrap()
                                            .text_color(if option.disabled {
                                                faint()
                                            } else {
                                                theme::TEXT()
                                            })
                                            .when(is_selected, |el| el.bg(theme::PANEL_HI()))
                                            .when(!option.disabled, |el| {
                                                el.cursor_pointer()
                                                    .hover(|style| style.bg(theme::PANEL_HI()))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.open_select = None;
                                                            on_pick(
                                                                this,
                                                                option.value,
                                                                window,
                                                                cx,
                                                            );
                                                            cx.notify();
                                                        },
                                                    ))
                                            })
                                            .child(option.label.clone())
                                    },
                                )),
                        ),
                    ))
                }),
        )
}
