//! The bottom strip: a five-across grid of per-driver stint cards — the port of
//! `frontend/src/components/StintTimeline.tsx`.

use gpui::{AnyElement, Context, Div, Hsla, Window, div, prelude::*, px, rems};
use interval_backend::domain::{DriverStatus, TyreCompound};
use interval_desktop_core::formatters;
use interval_desktop_core::stint_timeline::stint_progress_display;

use super::side_panels::{panel, tone_color};
use super::ui;
use crate::{IntervalApp, theme};

struct CardData {
    code: String,
    team_colour: Hsla,
    compound: TyreCompound,
    stint_age: Option<f64>,
    in_pit: bool,
    status: DriverStatus,
    last_lap: Option<f64>,
}

fn collect(app: &IntervalApp) -> Option<Vec<CardData>> {
    let store = app.store.state();
    let snapshot = store.active_snapshot()?;
    Some(
        snapshot
            .timing
            .rows
            .iter()
            .map(|row| CardData {
                code: row.driver.code.clone(),
                team_colour: theme::team_colour(&row.driver.team_colour),
                compound: row.compound.clone(),
                stint_age: row.stint_age.map(f64::from),
                in_pit: row.in_pit,
                status: row.status.clone(),
                last_lap: row.last_lap,
            })
            .collect(),
    )
}

pub fn stint_timeline(
    app: &mut IntervalApp,
    _window: &mut Window,
    _cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let body = match collect(app) {
        None => div().into_any_element(),
        Some(rows) => strip_body(&rows),
    };
    panel("RUN TIMELINE", None, body)
        .h_full()
        .min_h_0()
        .font_family(app.mono_font.clone())
}

fn strip_body(rows: &[CardData]) -> AnyElement {
    div()
        .id("stint-timeline-scroll")
        .size_full()
        .overflow_y_scroll()
        .p_2()
        .flex()
        .flex_col()
        .gap_2()
        .children(rows.chunks(5).map(|chunk| {
            let mut row = div().flex().flex_row().gap_2();
            for card in chunk {
                row = row.child(stint_card(card).flex_1());
            }
            // Fillers keep a partial last row on the five-column rhythm.
            for _ in chunk.len()..5 {
                row = row.child(div().flex_1());
            }
            row
        }))
        .into_any_element()
}

fn stint_card(card: &CardData) -> Div {
    let progress = stint_progress_display(card.stint_age);
    let compound_colour = tone_color(formatters::compound_class(&card.compound));
    let status_label = if card.in_pit {
        "IN PIT"
    } else {
        match card.status {
            DriverStatus::OnTrack => "ON_TRACK",
            DriverStatus::Pit => "PIT",
            DriverStatus::Out => "OUT",
        }
    };
    let bar_colour: Hsla = if !progress.known {
        gpui::rgb(0x334155).into() // slate-700, the unknown-age bar
    } else if card.in_pit {
        theme::DANGER()
    } else {
        theme::MINT()
    };

    div()
        .border_1()
        .border_color(theme::LINE())
        .bg(theme::STINT_CARD())
        .p_2()
        .text_size(rems(0.72))
        .child(
            div()
                .mb_1()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(div().flex_none().w(px(8.0)).h(px(8.0)).bg(card.team_colour))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(gpui::white())
                                .child(card.code.clone()),
                        ),
                )
                .child(
                    div()
                        .rounded_full()
                        .border_1()
                        .border_color(compound_colour)
                        .text_color(compound_colour)
                        .px(px(4.0))
                        .child(formatters::compound_abbreviation(&card.compound)),
                ),
        )
        .child(
            div()
                .mb_1()
                .flex()
                .flex_row()
                .justify_between()
                .text_color(ui::muted())
                .child(div().child(progress.label.clone()))
                .child(div().child(status_label)),
        )
        .child(
            div().h(px(16.0)).w_full().bg(theme::LINE()).child(
                div()
                    .h_full()
                    .w(gpui::relative((progress.width_percent / 100.0) as f32))
                    .bg(bar_colour),
            ),
        )
        .child(
            div()
                .mt_3()
                .text_color(theme::TIMING())
                .child(formatters::format_lap_time(card.last_lap)),
        )
}
