//! The bottom strip: a five-across grid of per-driver stint cards. Each card shows the
//! driver, tyre (compound + age), lap progress, and their last/fastest laps in the F1
//! pace colors — a per-driver summary rather than a copy of the timing table.

use gpui::{AnyElement, Context, Div, Hsla, Window, div, prelude::*, px, rems};
use interval_backend::domain::{DriverStatus, SectorStatus, TyreCompound};
use interval_desktop_core::formatters;

use super::side_panels::panel;
use super::ui;
use crate::{IntervalApp, theme};

struct CardData {
    code: String,
    team_colour: Hsla,
    compound: TyreCompound,
    stint_age: Option<i32>,
    lap: i32,
    position: i32,
    in_pit: bool,
    status: DriverStatus,
    last_lap: Option<f64>,
    last_lap_status: SectorStatus,
    best_lap: Option<f64>,
    best_lap_status: SectorStatus,
}

fn collect(app: &IntervalApp) -> Option<(Vec<CardData>, i32)> {
    let store = app.store.state();
    let snapshot = store.active_snapshot()?;
    let total_laps = store
        .display_metadata()
        .map(|metadata| metadata.session.total_laps)
        .unwrap_or(0);
    let cards = snapshot
        .timing
        .rows
        .iter()
        .filter(|row| !app.hidden_drivers.contains(&row.driver.driver_number))
        .map(|row| CardData {
            code: row.driver.code.clone(),
            team_colour: theme::team_colour(&row.driver.team_colour),
            compound: row.compound.clone(),
            stint_age: row.stint_age,
            lap: row.lap,
            position: row.position,
            in_pit: row.in_pit,
            status: row.status.clone(),
            last_lap: row.last_lap,
            last_lap_status: row.last_lap_status.clone(),
            best_lap: row.best_lap,
            best_lap_status: row.best_lap_status.clone(),
        })
        .collect();
    Some((cards, total_laps))
}

pub fn stint_timeline(
    app: &mut IntervalApp,
    _window: &mut Window,
    _cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let body = match collect(app) {
        None => div().into_any_element(),
        Some((rows, total_laps)) => strip_body(&rows, total_laps),
    };
    panel("RUN TIMELINE", body)
        .h_full()
        .min_h_0()
        .font_family(app.mono_font.clone())
}

fn strip_body(rows: &[CardData], total_laps: i32) -> AnyElement {
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
            // Every slot — card or filler — is the same bare `flex_1 min_w_0` div, so
            // the columns come out identical; the card fills its slot from inside.
            // (Putting border/padding on the flex item itself skewed the widths.)
            for card in chunk {
                row = row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(stint_card(card, total_laps).w_full()),
                );
            }
            // Fillers keep a partial last row on the five-column rhythm.
            for _ in chunk.len()..5 {
                row = row.child(div().flex_1().min_w_0());
            }
            row
        }))
        .into_any_element()
}

/// One label/value line in the card body.
fn metric_row(label: &'static str, value: String, value_color: Hsla) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .py(px(1.0))
        .child(div().text_color(ui::muted()).child(label))
        .child(div().text_color(value_color).child(value))
}

/// Lap-time value + color: pace-classed when present, muted "--" otherwise.
fn lap_metric(time: Option<f64>, status: &SectorStatus) -> (String, Hsla) {
    match time {
        Some(_) => (formatters::format_lap_time(time), ui::pace_color(status)),
        None => ("--".to_string(), ui::muted()),
    }
}

fn stint_card(card: &CardData, total_laps: i32) -> Div {
    let compound_colour = ui::tone_color(formatters::compound_class(&card.compound));
    let age_label = card
        .stint_age
        .map(|age| format!("{age} laps"))
        .unwrap_or_else(|| "--".to_string());
    let lap_label = if total_laps > 0 {
        format!("{}/{}", card.lap, total_laps)
    } else {
        format!("L{}", card.lap)
    };
    // Only exceptional states get a marker; ON_TRACK is the default and stays silent.
    let state_flag: Option<(&'static str, Hsla)> = if card.in_pit
        || card.status == DriverStatus::Pit
    {
        Some(("PIT", theme::DANGER()))
    } else if card.status == DriverStatus::Out {
        Some(("OUT", ui::muted()))
    } else {
        None
    };
    let (last_value, last_color) = lap_metric(card.last_lap, &card.last_lap_status);
    let (best_value, best_color) = lap_metric(card.best_lap, &card.best_lap_status);

    div()
        .border_1()
        .border_color(theme::LINE())
        .bg(theme::PANEL())
        .p_2()
        .text_size(rems(0.7))
        // Header: driver identity + tyre on the left, state marker on the right.
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
                        .min_w_0()
                        .child(div().flex_none().w(px(8.0)).h(px(8.0)).bg(card.team_colour))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(gpui::white())
                                .child(card.code.clone()),
                        )
                        .child(
                            div()
                                .rounded_full()
                                .border_1()
                                .border_color(compound_colour)
                                .text_color(compound_colour)
                                .px(px(4.0))
                                .text_size(rems(0.62))
                                .child(formatters::compound_abbreviation(&card.compound)),
                        )
                        .child(div().text_color(ui::muted()).child(age_label)),
                )
                .children(state_flag.map(|(label, color)| {
                    div()
                        .flex_none()
                        .text_size(rems(0.62))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(label)
                })),
        )
        .child(metric_row("POS", card.position.to_string(), theme::TEXT()))
        .child(metric_row("LAP", lap_label, theme::TEXT()))
        .child(metric_row("LAST", last_value, last_color))
        .child(metric_row("FAST", best_value, best_color))
}
