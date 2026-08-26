//! The timing tower: the P/DRV/GAP/INT/S1/S2/S3/LAP/TY table — the port of
//! `frontend/src/components/TimingTower.tsx` (with `Panel` and `QualityBadge` inlined).

use gpui::{Context, Hsla, Window, div, prelude::*, rems, uniform_list};
use interval_backend::domain::DriverSnapshot;
use interval_desktop_core::formatters::{self, Tone};
use interval_desktop_core::timing_display;

use super::ui;
use crate::{IntervalApp, theme};

/// Fixed column widths in rems, adapted from `.table-grid` (styles.css): sectors only
/// ever show `SS.mmm` (6 chars), while LAP shows `M:SS.mmm` (8 chars ≈ 4.2rem at this
/// font size) — the web's 3.2rem LAP track clipped it against the tyre column.
const COLUMNS: [f32; 9] = [2.2, 3.4, 4.4, 4.4, 4.0, 4.0, 4.0, 4.6, 3.1];
const HEADERS: [&str; 9] = ["P", "DRV", "GAP", "INT", "S1", "S2", "S3", "LAP", "TY"];
/// `.timing-cell` min-height, doubling as the uniform row height.
const ROW_HEIGHT: f32 = 1.65;

// Tailwind literals the web component's classes referenced directly.
const FUCHSIA: fn() -> Hsla = || gpui::rgb(0xe879f9).into();
const EMERALD: fn() -> Hsla = || gpui::rgb(0x34d399).into();
const SKY: fn() -> Hsla = || gpui::rgb(0x38bdf8).into();
const SLATE_100: fn() -> Hsla = || gpui::rgb(0xf1f5f9).into();
const SLATE_200: fn() -> Hsla = || gpui::rgb(0xe2e8f0).into();
const SLATE_300: fn() -> Hsla = || gpui::rgb(0xcbd5e1).into();

/// Lime marks a sub-second interval (DRS range); the pace palette lives in `ui`.
const DRS_LIME: fn() -> Hsla = || gpui::rgb(0xd7e34d).into();

/// The concrete color for a [`Tone`] — the CSS class strings the TS formatters
/// returned, resolved against the theme.
fn tone_color(tone: Tone) -> Hsla {
    match tone {
        Tone::Fuchsia => FUCHSIA(),
        Tone::Mint => theme::ACCENT(),
        Tone::Timing => theme::TIMING(),
        Tone::Danger => theme::DANGER(),
        Tone::Amber => theme::AMBER(),
        Tone::Bright => SLATE_100(),
        Tone::Emerald => EMERALD(),
        Tone::Sky => SKY(),
        Tone::Neutral => SLATE_300(),
        Tone::Muted => ui::muted(),
    }
}

/// One `.timing-cell`: fixed width, row height, `0.35rem` side padding, and the
/// half-strength hairline (`rgba(58,66,77,0.55)` — LINE at 0.55 over the panel).
fn cell(width: f32) -> gpui::Div {
    div()
        .w(rems(width))
        .flex_shrink_0()
        .h(rems(ROW_HEIGHT))
        .flex()
        .items_center()
        .px(rems(0.35))
        .border_b_1()
        .border_color(theme::blend(theme::LINE(), theme::PANEL(), 0.55))
        .whitespace_nowrap()
        .overflow_hidden()
}

/// `dimmed` — the driver is hidden by the transport-bar filter: every color collapses
/// to the faint grey so the row recedes without losing its slot.
fn timing_row(ix: usize, row: &DriverSnapshot, dimmed: bool) -> impl IntoElement + use<> {
    let paint = move |color: Hsla| if dimmed { ui::faint() } else { color };
    let compound_tone = paint(tone_color(formatters::compound_class(&row.compound)));
    div()
        .id(ix)
        .flex()
        .flex_row()
        .hover(|style| style.bg(theme::blend(theme::PANEL_HI(), theme::PANEL(), 0.8)))
        .child(
            cell(COLUMNS[0])
                .text_color(paint(theme::ACCENT()))
                .child(row.position.to_string()),
        )
        .child(
            cell(COLUMNS[1])
                .gap_1()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(paint(gpui::white()))
                .child(
                    div()
                        .h(rems(1.0))
                        .w(rems(0.25))
                        .flex_shrink_0()
                        .bg(if dimmed {
                            theme::blend(
                                theme::team_colour(&row.driver.team_colour),
                                theme::PANEL(),
                                0.3,
                            )
                        } else {
                            theme::team_colour(&row.driver.team_colour)
                        }),
                )
                .child(row.driver.code.clone()),
        )
        .child(
            cell(COLUMNS[2]).text_color(paint(theme::TIMING())).child(
                timing_display::gap_label(row.position, row.gap_to_leader.as_deref()).to_string(),
            ),
        )
        .child(
            cell(COLUMNS[3])
                .text_color(paint(
                    if timing_display::interval_within_one_second(row.interval.as_deref()) {
                        DRS_LIME()
                    } else {
                        SLATE_200()
                    },
                ))
                .child(timing_display::interval_label(row.interval.as_deref()).to_string()),
        )
        .children(
            timing_display::sector_cells(&row.sectors)
                .into_iter()
                .enumerate()
                .map(|(index, sector)| {
                    let (label, color) = match sector.and_then(|s| s.duration.map(|d| (s, d))) {
                        Some((sector, duration)) => {
                            (format!("{duration:.3}"), ui::pace_color(&sector.status))
                        }
                        None => ("--".to_string(), ui::muted()),
                    };
                    cell(COLUMNS[4 + index])
                        .text_color(paint(color))
                        .child(label)
                }),
        )
        .child(
            cell(COLUMNS[7])
                .text_color(paint(if row.last_lap.is_some() {
                    ui::pace_color(&row.last_lap_status)
                } else {
                    ui::muted()
                }))
                .child(formatters::format_lap_time(row.last_lap)),
        )
        .child(
            cell(COLUMNS[8]).child(
                div()
                    .rounded_full()
                    .border_1()
                    .border_color(compound_tone)
                    .text_color(compound_tone)
                    .px(rems(0.25))
                    .text_size(rems(0.62))
                    .child(formatters::compound_abbreviation(&row.compound)),
            ),
        )
}

pub fn timing_tower(
    app: &mut IntervalApp,
    _window: &mut Window,
    cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let row_count = {
        let store = app.store.state();
        store
            .active_snapshot()
            .map(|snapshot| snapshot.timing.rows.len())
            .unwrap_or(0)
    };

    div()
        .flex()
        .flex_col()
        .min_h_0()
        .overflow_hidden()
        .size_full()
        .border_1()
        .border_color(theme::LINE())
        .bg(theme::PANEL())
        // Slim panel header, matching `side_panels::panel`.
        .child(
            div()
                .h(rems(1.3))
                .flex_shrink_0()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme::LINE())
                .bg(theme::PANEL_HI())
                .px_2()
                .child(
                    div()
                        .text_size(rems(0.6))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::ACCENT())
                        .child("TIMING"),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .text_size(rems(0.73))
                // Column header row; lives outside the list, so it stays pinned the way
                // the web version's `sticky top-0` did.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_shrink_0()
                        .bg(theme::TIMING_HEADER())
                        .text_color(SLATE_300())
                        .children(COLUMNS.iter().zip(HEADERS).map(|(width, label)| {
                            cell(*width)
                                .border_color(theme::LINE())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(label)
                        })),
                )
                .child(if row_count == 0 {
                    div()
                        .px_2()
                        .py_3()
                        .text_color(ui::muted())
                        .child("No timing rows for this frame")
                        .into_any_element()
                } else {
                    uniform_list(
                        "timing-rows",
                        row_count,
                        cx.processor(
                            |this: &mut IntervalApp,
                             range: std::ops::Range<usize>,
                             _window,
                             _cx| {
                                let store = this.store.state();
                                let rows = store
                                    .active_snapshot()
                                    .map(|snapshot| snapshot.timing.rows.as_slice())
                                    .unwrap_or(&[]);
                                range
                                    .filter_map(|ix| {
                                        rows.get(ix).map(|row| {
                                            let dimmed = this
                                                .hidden_drivers
                                                .contains(&row.driver.driver_number);
                                            timing_row(ix, row, dimmed)
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            },
                        ),
                    )
                    .track_scroll(&app.timing_scroll)
                    .flex_grow(1.0)
                    .into_any_element()
                }),
        )
}
