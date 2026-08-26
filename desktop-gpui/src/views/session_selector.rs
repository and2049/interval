//! The top bar: Season/Meeting/Session dropdowns, readiness badge, the OPEN/INGEST
//! action, OPEN LIVE, the live-availability badge, and transient status text — the
//! port of `frontend/src/components/SessionSelector.tsx`'s markup (its state machine
//! lives in `interval_desktop_core::selector`).

use gpui::{Context, SharedString, Window, div, prelude::*, px, rems};
use interval_desktop_core::replay_quality::BadgeTone;
use interval_desktop_core::session_readiness::{self, SessionActionState};

use super::ui::{self, SelectKind, SelectOption};
use crate::{IntervalApp, theme};

pub fn session_selector(
    app: &mut IntervalApp,
    window: &mut Window,
    cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let settings_cell =
        super::settings_menu(app, window, cx).map(|element| element.into_any_element());
    let (live_checking, live_active, live_status_message) = {
        let store = app.store.state();
        (
            store.live_availability_checking,
            store.live_active,
            store.live_availability_message.clone(),
        )
    };
    let active_session_key = app.store.state().session_key;

    let selector = app.selector.state();
    let selected_season = selector.selected_season;
    let selected_meeting = selector.selected_meeting;
    let selected_session = selector.selected_session;
    let ingest_state = selector.ingest_state();
    let seasons_loading = selector.seasons.loading;
    let meetings_loading = selector.meetings.loading;
    let sessions_loading = selector.sessions.loading;

    let season_options: Vec<SelectOption<i32>> = selector
        .season_options()
        .into_iter()
        .map(|option| SelectOption {
            value: option.value as i32,
            label: SharedString::from(option.label),
            disabled: option.disabled,
        })
        .collect();
    let meeting_options: Vec<SelectOption<i64>> = selector
        .meeting_options()
        .into_iter()
        .map(|option| SelectOption {
            value: option.value,
            label: SharedString::from(option.label),
            disabled: option.disabled,
        })
        .collect();
    let session_options: Vec<SelectOption<i64>> = selector
        .session_options()
        .into_iter()
        .map(|option| SelectOption {
            value: option.value,
            label: SharedString::from(option.label),
            disabled: option.disabled,
        })
        .collect();

    let readiness_badge = selector.selected_readiness().map(|entry| {
        ui::badge(
            session_readiness::session_status_badge_text(entry),
            ui::readiness_tone_color(session_readiness::session_status_class(
                &entry.ingest_status,
            )),
        )
    });

    let action_label = session_readiness::session_action_label(
        ingest_state,
        selected_session,
        active_session_key,
        selector.selected_readiness(),
    );
    let action_disabled = session_readiness::is_session_action_disabled(
        selected_session,
        selector.selected_readiness(),
        ingest_state,
        live_active,
    );
    let action_status = session_readiness::session_action_status(ingest_state);
    let ingest_failed_message = (ingest_state == SessionActionState::Failed).then(|| {
        session_readiness::session_ingest_error_message(
            selector.ingest_error.as_deref(),
            selector.selected_readiness(),
        )
    });
    let latest_outcome = session_readiness::ingest_outcome(selector.last_ingest.as_ref());
    let discovery_failed = selector.discovery_failed();
    let no_race_session = selector.no_race_session_for_meeting();

    let value_label = |options: &[SelectOption<i64>], selected: Option<i64>| -> SharedString {
        selected
            .and_then(|value| {
                options
                    .iter()
                    .find(|option| option.value == value)
                    .map(|option| option.label.clone())
            })
            .unwrap_or_else(|| SharedString::from("—"))
    };
    let season_label: SharedString = selected_season
        .map(|year| SharedString::from(year.to_string()))
        .unwrap_or_else(|| SharedString::from("—"));
    let meeting_label = value_label(&meeting_options, selected_meeting);
    let session_label = value_label(&session_options, selected_session);
    drop(selector);

    let selector_locked = live_active;

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .border_b_1()
        .border_color(theme::LINE())
        .bg(theme::BAR_BG_DEEP())
        .px_3()
        .py_2()
        .font_family(app.mono_font.clone())
        .text_size(rems(0.72))
        .child(ui::select_field(
            "season-select",
            SelectKind::Season,
            "Season",
            season_label,
            season_options,
            selected_season,
            seasons_loading || selector_locked,
            |this, season, _window, _cx| this.selector.choose_season(season),
            app,
            cx,
        ))
        .child(ui::select_field(
            "meeting-select",
            SelectKind::Meeting,
            "Meeting",
            meeting_label,
            meeting_options,
            selected_meeting,
            meetings_loading || selector_locked,
            |this, meeting, _window, _cx| this.selector.choose_meeting(meeting),
            app,
            cx,
        ))
        .child(ui::select_field(
            "session-select",
            SelectKind::Session,
            "Session",
            session_label,
            session_options,
            selected_session,
            sessions_loading || selector_locked,
            |this, session, _window, _cx| this.selector.choose_session(session),
            app,
            cx,
        ))
        .children(readiness_badge)
        .child(
            // The OPEN / INGEST + OPEN / RELOAD action.
            div()
                .id("session-open")
                .ml_2()
                .border_1()
                .px_3()
                .py(px(3.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .map(|el| {
                    if action_disabled {
                        el.border_color(theme::LINE()).text_color(ui::faint())
                    } else {
                        el.border_color(theme::ACCENT())
                            .bg(theme::blend(theme::ACCENT(), theme::BAR_BG_DEEP(), 0.1))
                            .text_color(theme::ACCENT())
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _window, _cx| {
                                this.selector.open_selected();
                            }))
                    }
                })
                .child(action_label),
        )
        .child(
            // OPEN LIVE / CHECKING LIVE / LIVE OPEN.
            div()
                .id("live-check")
                .border_1()
                .px_3()
                .py(px(3.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .map(|el| {
                    if live_checking || live_active {
                        el.border_color(theme::LINE()).text_color(ui::faint())
                    } else {
                        el.border_color(theme::LINE())
                            .bg(theme::PANEL())
                            .text_color(ui::muted())
                            .cursor_pointer()
                            .hover(|style| {
                                style
                                    .border_color(theme::ACCENT())
                                    .text_color(theme::ACCENT())
                            })
                            .on_click(cx.listener(|this, _, _window, _cx| {
                                this.store.check_live();
                            }))
                    }
                })
                .child(if live_active {
                    "LIVE OPEN"
                } else if live_checking {
                    "CHECKING LIVE"
                } else {
                    "OPEN LIVE"
                }),
        )
        .children(
            action_status.map(|message| div().text_color(theme::AMBER()).child(message)),
        )
        .when(live_active, |el| {
            el.child(
                div()
                    .text_color(theme::ACCENT())
                    .child("Live race owns the dashboard."),
            )
        })
        .children(ingest_failed_message.map(|message| {
            div()
                .text_color(theme::DANGER())
                .whitespace_nowrap()
                .child(message)
        }))
        .when(no_race_session, |el| {
            el.child(
                div()
                    .text_color(theme::AMBER())
                    .child("No race session available for this meeting."),
            )
        })
        .children(latest_outcome.map(|outcome| {
            div()
                .max_w(rems(28.0))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(ui::readiness_tone_color(
                    session_readiness::ingest_outcome_class(outcome.tone),
                ))
                .child(outcome.label)
        }))
        .when(discovery_failed, |el| {
            el.child(
                div()
                    .text_color(theme::DANGER())
                    .child("OpenF1 discovery failed."),
            )
        })
        .children(live_status_message.map(|message| {
            div()
                .text_color(ui::muted())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(message)
        }))
        .child(div().flex_1())
        .children(settings_cell)
}

/// Which badge tone the live-availability chip uses, re-exported for tests later.
#[allow(dead_code)]
pub fn live_badge_tone(tone: BadgeTone) -> gpui::Hsla {
    ui::badge_tone_color(tone)
}
