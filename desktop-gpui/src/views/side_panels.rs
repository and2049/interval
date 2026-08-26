//! The right-hand stack: Race Control, Event Feed, Weather, Derived Metrics — the port
//! of `frontend/src/components/SidePanels.tsx` and its four panel children.

use gpui::{AnyElement, Context, Div, Hsla, Window, div, prelude::*, px, rems};
use interval_backend::domain::{RaceControlMessage, ReplayEvent};
use interval_desktop_core::derived_metrics::{DerivedMetricDisplay, derived_metric_rows};
use interval_desktop_core::formatters::{self, Tone};
use interval_desktop_core::replay_events::{
    EventFeedState, event_feed_empty_label, event_feed_state, event_kind_label,
    event_severity_class, recent_replay_events,
};
use interval_desktop_core::weather_display::{
    WeatherMetricDisplay, has_weather_sample, weather_metrics,
};

use super::ui;
use crate::{IntervalApp, theme};

/// Concrete colors for `formatters::Tone`. Tailwind hexes for the classes outside the
/// theme palette (fuchsia/emerald/sky/slate).
pub(crate) fn tone_color(tone: Tone) -> Hsla {
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
        Tone::Muted => ui::muted(),
    }
}

/// The `.panel` section frame with a slim uppercase mono header — deliberately lower
/// profile than the web `Panel.tsx` (which spent a 2rem bar plus a quality badge on it).
pub(crate) fn panel(title: &'static str, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_1()
        .border_color(theme::LINE())
        .bg(theme::PANEL())
        .child(
            div()
                .flex()
                .items_center()
                .flex_none()
                .h(rems(1.3))
                .border_b_1()
                .border_color(theme::LINE())
                .bg(theme::PANEL_HI())
                .px_2()
                .child(
                    div()
                        .text_size(rems(0.6))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::ACCENT())
                        .child(title),
                ),
        )
        .child(div().flex_1().min_h_0().overflow_hidden().child(body))
}

fn empty_state(label: &'static str) -> Div {
    div().py_3().text_color(ui::faint()).child(label)
}

/// `border-line/70` and `/60` row separators, flattened over the panel background.
fn separator(alpha: f32) -> Hsla {
    theme::blend(theme::LINE(), theme::PANEL(), alpha)
}

struct PanelsData {
    race_control: Vec<RaceControlMessage>,
    events: Vec<ReplayEvent>,
    events_state: EventFeedState,
    weather: Option<Vec<WeatherMetricDisplay>>,
    derived: Vec<DerivedMetricDisplay>,
}

fn collect(app: &IntervalApp) -> Option<PanelsData> {
    let store = app.store.state();
    let snapshot = store.active_snapshot()?;
    let events = recent_replay_events(store.active_events(), snapshot.cursor.t, 5);
    let events_state = event_feed_state(
        &events,
        store.active_events_loading(),
        store.active_events_error().is_some(),
    );
    Some(PanelsData {
        race_control: snapshot.race_control.messages.clone(),
        events,
        events_state,
        weather: has_weather_sample(&snapshot.weather)
            .then(|| weather_metrics(&snapshot.weather)),
        derived: derived_metric_rows(&snapshot.derived_metrics, &snapshot.timing.rows),
    })
}

pub fn side_panels(
    app: &mut IntervalApp,
    _window: &mut Window,
    _cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let data = collect(app);
    let column = div()
        .flex()
        .flex_col()
        .gap_2()
        .h_full()
        .min_h_0()
        .font_family(app.mono_font.clone());
    let Some(data) = data else {
        return column
            .child(panel("RACE CONTROL", div()).flex_1().min_h(rems(6.0)))
            .child(panel("EVENT FEED", div()).flex_1().min_h(rems(6.0)))
            .child(panel("WEATHER", div()).flex_none().h(rems(7.0)))
            .child(panel("DERIVED METRICS", div()).flex_1().min_h(rems(6.0)));
    };
    column
        .child(
            panel("RACE CONTROL", race_control_body(&data.race_control))
                .flex_1()
                .min_h(rems(6.0)),
        )
        .child(
            panel("EVENT FEED", event_feed_body(&data.events, data.events_state))
                .flex_1()
                .min_h(rems(6.0)),
        )
        .child(
            panel("WEATHER", weather_body(data.weather.as_deref()))
                .flex_none()
                .h(rems(7.0)),
        )
        .child(
            panel("DERIVED METRICS", derived_body(&data.derived))
                .flex_1()
                .min_h(rems(6.0)),
        )
}

fn race_control_body(messages: &[RaceControlMessage]) -> AnyElement {
    if messages.is_empty() {
        return div()
            .p_2()
            .text_size(rems(0.7))
            .child(empty_state("No race-control messages at this time"))
            .into_any_element();
    }
    div()
        .id("race-control-scroll")
        .size_full()
        .overflow_y_scroll()
        .p_2()
        .text_size(rems(0.7))
        .children(messages.iter().map(|event| {
            div()
                .mb_2()
                .pb_2()
                .border_b_1()
                .border_color(separator(0.7))
                .child(div().text_color(ui::muted()).child(format!(
                    "{} · {}",
                    formatters::format_event_clock(event.t),
                    event.category
                )))
                .child(
                    div()
                        .text_color(if event.flag.as_deref() == Some("yellow") {
                            theme::AMBER()
                        } else {
                            theme::TEXT()
                        })
                        .child(event.message.clone()),
                )
        }))
        .into_any_element()
}

fn event_feed_body(events: &[ReplayEvent], state: EventFeedState) -> AnyElement {
    if state != EventFeedState::Ready {
        return div()
            .p_2()
            .text_size(rems(0.7))
            .child(empty_state(event_feed_empty_label(state)))
            .into_any_element();
    }
    div()
        .id("event-feed-scroll")
        .size_full()
        .overflow_y_scroll()
        .p_2()
        .text_size(rems(0.7))
        .children(events.iter().map(|event| {
            div()
                .mb_2()
                .pb_2()
                .border_b_1()
                .border_color(separator(0.7))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .text_color(ui::muted())
                        .child(div().child(formatters::format_event_clock(event.t)))
                        .child(
                            div()
                                .text_color(tone_color(event_severity_class(&event.severity)))
                                .child(event_kind_label(&event.kind)),
                        ),
                )
                .child(
                    div()
                        .mt(px(2.0))
                        .text_color(theme::TEXT())
                        .child(event.message.clone()),
                )
        }))
        .into_any_element()
}

fn weather_body(metrics: Option<&[WeatherMetricDisplay]>) -> AnyElement {
    let Some(metrics) = metrics else {
        return div()
            .p_2()
            .text_size(rems(0.7))
            .child(empty_state("No weather sample for this frame"))
            .into_any_element();
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .justify_center()
        .gap_2()
        .p_2()
        .text_size(rems(0.7))
        .children(metrics.chunks(2).map(|pair| {
            div().flex().flex_row().gap_4().children(pair.iter().map(|metric| {
                div()
                    .flex_1()
                    .child(div().text_color(ui::muted()).child(metric.label))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT())
                            .child(metric.value.clone()),
                    )
            }))
        }))
        .into_any_element()
}

fn derived_body(rows: &[DerivedMetricDisplay]) -> AnyElement {
    if rows.is_empty() {
        return div()
            .p_2()
            .text_size(rems(0.7))
            .child(empty_state("No derived metrics for this frame"))
            .into_any_element();
    }
    div()
        .id("derived-metrics-scroll")
        .size_full()
        .overflow_y_scroll()
        .p_2()
        .text_size(rems(0.7))
        .children(rows.iter().map(|row| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .py_1()
                .border_b_1()
                .border_color(separator(0.6))
                .child(
                    div()
                        .flex_none()
                        .w(rems(3.0))
                        .text_color(ui::muted())
                        .child(row.driver.clone()),
                )
                .child(div().flex_1().min_w_0().child(row.label.clone()))
                .child(
                    div()
                        .flex_none()
                        .w(rems(4.0))
                        .text_color(tone_color(formatters::trend_class(&row.trend)))
                        .child(row.value.clone()),
                )
        }))
        .into_any_element()
}
