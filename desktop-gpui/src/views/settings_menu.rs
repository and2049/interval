//! The OpenF1 token settings popover — the port of
//! `frontend/src/components/SettingsMenu.tsx`, including its hand-rolled password
//! input (gpui has no text field; echo's pattern: a focusable div, a String buffer,
//! and key handling done by hand).

use gpui::{Context, FocusHandle, SharedString, Window, div, prelude::*, px, rems, svg};
use interval_desktop_core::settings_panel::{
    self, OpenF1TokenProbe, OpenF1TokenSettings, is_submittable_token,
};

use super::ui;
use crate::{IntervalApp, theme};

/// UI state for the popover. The settings themselves are `None` until the initial
/// fetch settles, and `Some(None)` when the routes are unavailable (a web deployment),
/// which hides the gear entirely.
pub struct SettingsUi {
    pub open: bool,
    pub token_input: String,
    pub reveal: bool,
    pub busy: bool,
    pub probe: Option<OpenF1TokenProbe>,
    pub error: Option<String>,
    pub settings: Option<Option<OpenF1TokenSettings>>,
    pub focus: FocusHandle,
}

impl SettingsUi {
    pub fn new(cx: &mut Context<IntervalApp>) -> Self {
        Self {
            open: false,
            token_input: String::new(),
            reveal: false,
            busy: false,
            probe: None,
            error: None,
            settings: None,
            focus: cx.focus_handle(),
        }
    }
}

/// The chord every platform's user reaches for to paste; a Mac that only matched
/// `control` would make the token input impossible to paste into.
fn is_paste_chord(modifiers: &gpui::Modifiers) -> bool {
    modifiers.secondary() || modifiers.control
}

/// The gear cell for the selector bar: `None` while the settings probe is pending or
/// the routes are absent.
pub fn settings_menu(
    app: &mut IntervalApp,
    _window: &mut Window,
    cx: &mut Context<IntervalApp>,
) -> Option<impl IntoElement> {
    let current = app.settings.settings.as_ref()?.clone()?;
    let open = app.settings.open;
    let busy = app.settings.busy;
    let reveal = app.settings.reveal;
    let token_input = app.settings.token_input.clone();
    let probe = app.settings.probe.clone();
    let error = app.settings.error.clone();

    let source_line = settings_panel::token_source_line(&current);
    let env_notice = settings_panel::env_override_notice(&current);
    let probe_badge = probe.as_ref().map(settings_panel::probe_badge);
    let can_save = !busy && is_submittable_token(&token_input);
    let can_test = !busy && current.configured;
    let can_clear = !busy
        && current.source == settings_panel::OpenF1TokenSource::Settings;

    let action_button = |id: &'static str,
                         label: &'static str,
                         enabled: bool,
                         accent: bool,
                         cx: &mut Context<IntervalApp>,
                         on_click: fn(&mut IntervalApp, &mut Context<IntervalApp>)| {
        div()
            .id(id)
            .border_1()
            .px_3()
            .py(px(3.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .map(|el| {
                if !enabled {
                    el.border_color(theme::LINE()).text_color(ui::faint())
                } else if accent {
                    el.border_color(theme::MINT())
                        .bg(theme::blend(theme::MINT(), theme::PANEL(), 0.1))
                        .text_color(theme::MINT())
                        .cursor_pointer()
                } else {
                    el.border_color(theme::LINE())
                        .text_color(ui::muted())
                        .cursor_pointer()
                        .hover(|style| {
                            style.border_color(theme::MINT()).text_color(theme::MINT())
                        })
                }
            })
            .when(enabled, |el| {
                el.on_click(cx.listener(move |this, _, _window, cx| on_click(this, cx)))
            })
            .child(label)
    };

    let masked: SharedString = if reveal {
        SharedString::from(token_input.clone())
    } else {
        SharedString::from("•".repeat(token_input.chars().count()))
    };
    let placeholder: Option<SharedString> = token_input.is_empty().then(|| {
        if current.configured {
            SharedString::from(current.hint.clone().unwrap_or_default())
        } else {
            SharedString::from("Paste your OpenF1 token")
        }
    });
    let input_focused = app.settings.focus.is_focused(_window);

    Some(
        div()
            .relative()
            .flex()
            .items_center()
            .child(
                div()
                    .id("settings-toggle")
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(theme::LINE())
                    .p_2()
                    .cursor_pointer()
                    .hover(|style| style.border_color(theme::MINT()))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.settings.open = !this.settings.open;
                        if this.settings.open {
                            window.focus(&this.settings.focus, cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path("icons/settings.svg")
                            .size(px(13.0))
                            .text_color(ui::muted()),
                    ),
            )
            .when(open, |el| {
                el.child(gpui::deferred(
                    gpui::anchored()
                        .snap_to_window_with_margin(px(8.0))
                        .child(
                            div()
                                .id("settings-panel")
                                .occlude()
                                .mt(px(4.0))
                                .w(rems(24.0))
                                .border_1()
                                .border_color(theme::LINE())
                                .bg(theme::PANEL())
                                .p_3()
                                .shadow_lg()
                                .text_size(rems(0.72))
                                .on_mouse_down_out(cx.listener(|this, _, _window, cx| {
                                    this.settings.open = false;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .mb_2()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(theme::MINT())
                                        .child("OPENF1 API TOKEN"),
                                )
                                .child(
                                    div()
                                        .mb_2()
                                        .text_color(ui::muted())
                                        .child(source_line),
                                )
                                .child(
                                    div()
                                        .mb_2()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            // The hand-rolled password input.
                                            div()
                                                .id("settings-token-input")
                                                .key_context("settings_input")
                                                .track_focus(&app.settings.focus)
                                                .flex_1()
                                                .border_1()
                                                .border_color(if input_focused {
                                                    theme::MINT()
                                                } else {
                                                    theme::LINE()
                                                })
                                                .bg(theme::PANEL())
                                                .px_2()
                                                .py_1()
                                                .cursor_text()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    window.focus(&this.settings.focus, cx);
                                                    cx.notify();
                                                }))
                                                .on_key_down(cx.listener(
                                                    |this, event: &gpui::KeyDownEvent, window, cx| {
                                                        handle_input_key(this, event, window, cx);
                                                    },
                                                ))
                                                .child(match placeholder {
                                                    Some(placeholder) => div()
                                                        .text_color(ui::faint())
                                                        .child(placeholder)
                                                        .into_any_element(),
                                                    None => div()
                                                        .flex()
                                                        .flex_row()
                                                        .items_center()
                                                        .child(masked)
                                                        .when(input_focused, |el| {
                                                            el.child(
                                                                div()
                                                                    .w(px(1.0))
                                                                    .h(px(14.0))
                                                                    .bg(theme::MINT()),
                                                            )
                                                        })
                                                        .into_any_element(),
                                                }),
                                        )
                                        .child(
                                            div()
                                                .id("settings-reveal")
                                                .border_1()
                                                .border_color(theme::LINE())
                                                .px_2()
                                                .py_1()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(ui::muted())
                                                .cursor_pointer()
                                                .hover(|style| {
                                                    style
                                                        .border_color(theme::MINT())
                                                        .text_color(theme::MINT())
                                                })
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    this.settings.reveal =
                                                        !this.settings.reveal;
                                                    cx.notify();
                                                }))
                                                .child(if reveal { "HIDE" } else { "SHOW" }),
                                        ),
                                )
                                .child(
                                    div()
                                        .mb_2()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .child(action_button(
                                            "settings-save",
                                            "SAVE",
                                            can_save,
                                            true,
                                            cx,
                                            |this, cx| this.settings_save(cx),
                                        ))
                                        .child(action_button(
                                            "settings-test",
                                            "TEST",
                                            can_test,
                                            false,
                                            cx,
                                            |this, cx| this.settings_test(cx),
                                        ))
                                        .child(action_button(
                                            "settings-clear",
                                            "CLEAR",
                                            can_clear,
                                            false,
                                            cx,
                                            |this, cx| this.settings_clear(cx),
                                        ))
                                        .children(
                                            probe_badge
                                                .as_ref()
                                                .map(|badge| ui::channel_badge(badge)),
                                        ),
                                )
                                .children(probe.as_ref().map(|probe| {
                                    div()
                                        .mb_2()
                                        .text_color(ui::muted())
                                        .child(probe.message.clone())
                                }))
                                .children(error.map(|message| {
                                    div().mb_2().text_color(theme::DANGER()).child(message)
                                }))
                                .children(env_notice.map(|notice| {
                                    div().mb_2().text_color(theme::AMBER()).child(notice)
                                }))
                                .children(current.path.clone().map(|path| {
                                    div()
                                        .text_size(rems(0.62))
                                        .text_color(ui::faint())
                                        .child(format!("Stored in {path}"))
                                })),
                        ),
                ))
            }),
    )
}

fn handle_input_key(
    this: &mut IntervalApp,
    event: &gpui::KeyDownEvent,
    _window: &mut Window,
    cx: &mut Context<IntervalApp>,
) {
    let keystroke = &event.keystroke;
    match keystroke.key.as_str() {
        "escape" => {
            this.settings.open = false;
            cx.notify();
            return;
        }
        "backspace" => {
            this.settings.token_input.pop();
            cx.notify();
            return;
        }
        "enter" => {
            if !this.settings.busy && is_submittable_token(&this.settings.token_input) {
                this.settings_save(cx);
            }
            return;
        }
        "v" if is_paste_chord(&keystroke.modifiers) => {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                this.settings.token_input.push_str(text.trim());
                cx.notify();
            }
            return;
        }
        _ => {}
    }
    if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
        return;
    }
    if let Some(key_char) = keystroke.key_char.as_deref() {
        // Tokens have no meaningful whitespace; ignore everything unprintable.
        if !key_char.chars().any(char::is_control) {
            this.settings.token_input.push_str(key_char);
            cx.notify();
        }
    }
}
