//! The OpenF1 account settings popover — the port of
//! `frontend/src/components/SettingsMenu.tsx`, including its hand-rolled text
//! inputs (gpui has no text field; echo's pattern: a focusable div, a String buffer,
//! and key handling done by hand).
//!
//! OpenF1 issues one-hour tokens against an account, so the panel asks for the
//! account rather than a token; the backend exchanges it and re-exchanges on expiry.

use gpui::{Context, FocusHandle, SharedString, Window, div, prelude::*, px, rems, svg};
use interval_desktop_core::settings_panel::{
    self, OpenF1LoginProbe, OpenF1LoginSettings, is_submittable_login,
};

use super::ui;
use crate::{IntervalApp, theme, updates::UpdateState};

/// UI state for the popover. The settings themselves are `None` until the initial
/// fetch settles, and `Some(None)` when the routes are unavailable (a web deployment),
/// which hides the gear entirely.
pub struct SettingsUi {
    pub open: bool,
    pub username_input: String,
    pub password_input: String,
    pub reveal: bool,
    pub busy: bool,
    pub probe: Option<OpenF1LoginProbe>,
    pub error: Option<String>,
    pub settings: Option<Option<OpenF1LoginSettings>>,
    pub username_focus: FocusHandle,
    pub password_focus: FocusHandle,
}

impl SettingsUi {
    pub fn new(cx: &mut Context<IntervalApp>) -> Self {
        Self {
            open: false,
            username_input: String::new(),
            password_input: String::new(),
            reveal: false,
            busy: false,
            probe: None,
            error: None,
            settings: None,
            username_focus: cx.focus_handle(),
            password_focus: cx.focus_handle(),
        }
    }

    pub fn can_submit(&self) -> bool {
        !self.busy && is_submittable_login(&self.username_input, &self.password_input)
    }

    fn buffer_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Username => &mut self.username_input,
            Field::Password => &mut self.password_input,
        }
    }

    fn focus_handle(&self, field: Field) -> &FocusHandle {
        match field {
            Field::Username => &self.username_focus,
            Field::Password => &self.password_focus,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Username,
    Password,
}

impl Field {
    fn other(self) -> Self {
        match self {
            Field::Username => Field::Password,
            Field::Password => Field::Username,
        }
    }
}

/// The chord every platform's user reaches for to paste; a Mac that only matched
/// `control` would make the inputs impossible to paste into.
fn is_paste_chord(modifiers: &gpui::Modifiers) -> bool {
    modifiers.secondary() || modifiers.control
}

/// The gear cell for the selector bar: `None` while the settings probe is pending or
/// the routes are absent.
pub fn settings_menu(
    app: &mut IntervalApp,
    window: &mut Window,
    cx: &mut Context<IntervalApp>,
) -> Option<impl IntoElement> {
    let current = app.settings.settings.as_ref()?.clone()?;
    let open = app.settings.open;
    let busy = app.settings.busy;
    let reveal = app.settings.reveal;
    let username_input = app.settings.username_input.clone();
    let password_input = app.settings.password_input.clone();
    let probe = app.settings.probe.clone();
    let error = app.settings.error.clone();

    let source_line = settings_panel::login_source_line(&current);
    let env_notice = settings_panel::env_override_notice(&current);
    let probe_badge = probe.as_ref().map(settings_panel::probe_badge);
    let can_save = app.settings.can_submit();
    let can_test = !busy && current.configured;
    let can_clear = !busy && current.source == settings_panel::OpenF1AuthSource::Settings;

    let action_button = |id: &'static str,
                         label: SharedString,
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
                    el.border_color(theme::ACCENT())
                        .bg(theme::blend(theme::ACCENT(), theme::PANEL(), 0.1))
                        .text_color(theme::ACCENT())
                        .cursor_pointer()
                } else {
                    el.border_color(theme::LINE())
                        .text_color(ui::muted())
                        .cursor_pointer()
                        .hover(|style| {
                            style.border_color(theme::ACCENT()).text_color(theme::ACCENT())
                        })
                }
            })
            .when(enabled, |el| {
                el.on_click(cx.listener(move |this, _, _window, cx| on_click(this, cx)))
            })
            .child(label)
    };

    let username_placeholder: Option<SharedString> = username_input.is_empty().then(|| {
        match current.username.as_deref().filter(|name| !name.is_empty()) {
            Some(name) => SharedString::from(name.to_string()),
            None => SharedString::from("OpenF1 account email"),
        }
    });
    let password_placeholder: Option<SharedString> = password_input
        .is_empty()
        .then(|| SharedString::from("Password"));
    let password_display: SharedString = if reveal {
        SharedString::from(password_input.clone())
    } else {
        SharedString::from("•".repeat(password_input.chars().count()))
    };

    let username_field = text_input(
        app,
        window,
        cx,
        Field::Username,
        SharedString::from(username_input),
        username_placeholder,
    );
    let password_field = text_input(
        app,
        window,
        cx,
        Field::Password,
        password_display,
        password_placeholder,
    );

    let (update_label, update_hint, update_active) = app.update_state.presentation();
    let update_available = matches!(app.update_state, UpdateState::Available(_));
    let version_line = match interval_desktop_core::update::current_version() {
        Some(version) => format!("Version {version}"),
        None => "Development build".to_string(),
    };
    let update_section = div()
        .mt_2()
        .pt_2()
        .border_t_1()
        .border_color(theme::LINE())
        .child(
            div()
                .mb_2()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::ACCENT())
                .child("APP"),
        )
        .child(div().mb_2().text_color(ui::muted()).child(version_line))
        .child(action_button(
            "settings-update",
            update_label.into(),
            update_active,
            update_available,
            cx,
            |this, cx| this.update_button_clicked(cx),
        ))
        .children(update_hint.map(|hint| {
            div()
                .mt_2()
                .text_size(rems(0.62))
                .text_color(ui::muted())
                .child(hint)
        }));

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
                    .border_color(if update_available {
                        theme::ACCENT()
                    } else {
                        theme::LINE()
                    })
                    .p_2()
                    .cursor_pointer()
                    .hover(|style| style.border_color(theme::ACCENT()))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.settings.open = !this.settings.open;
                        if this.settings.open {
                            window.focus(&this.settings.username_focus, cx);
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
                                        .text_color(theme::ACCENT())
                                        .child("OPENF1 ACCOUNT"),
                                )
                                .child(
                                    div()
                                        .mb_2()
                                        .text_color(ui::muted())
                                        .child(source_line),
                                )
                                .child(div().mb_2().child(username_field))
                                .child(
                                    div()
                                        .mb_2()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .child(password_field)
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
                                                        .border_color(theme::ACCENT())
                                                        .text_color(theme::ACCENT())
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
                                            "SIGN IN".into(),
                                            can_save,
                                            true,
                                            cx,
                                            |this, cx| this.settings_save(cx),
                                        ))
                                        .child(action_button(
                                            "settings-test",
                                            "TEST".into(),
                                            can_test,
                                            false,
                                            cx,
                                            |this, cx| this.settings_test(cx),
                                        ))
                                        .child(action_button(
                                            "settings-clear",
                                            "SIGN OUT".into(),
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
                                .child(
                                    div()
                                        .mb_1()
                                        .text_size(rems(0.62))
                                        .text_color(ui::faint())
                                        .child(
                                            "OpenF1 tokens last an hour; the app signs in again on its own.",
                                        ),
                                )
                                .children(current.path.clone().map(|path| {
                                    div()
                                        .text_size(rems(0.62))
                                        .text_color(ui::faint())
                                        .child(format!("Stored in {path}"))
                                }))
                                .child(update_section),
                        ),
                ))
            }),
    )
}

/// One hand-rolled input. `display` is what to draw (already masked for the password),
/// `placeholder` what to draw instead when the buffer is empty.
fn text_input(
    app: &IntervalApp,
    window: &Window,
    cx: &mut Context<IntervalApp>,
    field: Field,
    display: SharedString,
    placeholder: Option<SharedString>,
) -> gpui::AnyElement {
    let focus = app.settings.focus_handle(field).clone();
    let focused = focus.is_focused(window);
    let id: &'static str = match field {
        Field::Username => "settings-username-input",
        Field::Password => "settings-password-input",
    };
    div()
        .id(id)
        .key_context("settings_input")
        .track_focus(&focus)
        .flex_1()
        .border_1()
        .border_color(if focused {
            theme::ACCENT()
        } else {
            theme::LINE()
        })
        .bg(theme::PANEL())
        .px_2()
        .py_1()
        .cursor_text()
        .on_click(cx.listener(move |this, _, window, cx| {
            window.focus(this.settings.focus_handle(field), cx);
            cx.notify();
        }))
        .on_key_down(cx.listener(
            move |this, event: &gpui::KeyDownEvent, window, cx| {
                handle_input_key(this, field, event, window, cx);
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
                .child(display)
                .when(focused, |el| {
                    el.child(div().w(px(1.0)).h(px(14.0)).bg(theme::ACCENT()))
                })
                .into_any_element(),
        })
        .into_any_element()
}

fn handle_input_key(
    this: &mut IntervalApp,
    field: Field,
    event: &gpui::KeyDownEvent,
    window: &mut Window,
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
            this.settings.buffer_mut(field).pop();
            cx.notify();
            return;
        }
        // Two fields, so tab and shift-tab both mean "the other one".
        "tab" => {
            window.focus(this.settings.focus_handle(field.other()), cx);
            cx.notify();
            return;
        }
        "enter" => {
            if this.settings.can_submit() {
                this.settings_save(cx);
            } else if field == Field::Username {
                // Enter on a filled email is "next field", as in a browser form.
                window.focus(&this.settings.password_focus, cx);
                cx.notify();
            }
            return;
        }
        "v" if is_paste_chord(&keystroke.modifiers) => {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                // A pasted email never wants its surrounding whitespace; a pasted
                // password is taken as-is, since OpenF1 generated it.
                let text = match field {
                    Field::Username => text.trim().to_string(),
                    Field::Password => text,
                };
                this.settings.buffer_mut(field).push_str(&text);
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
        // Neither field has meaningful newlines or tabs; ignore everything unprintable.
        if !key_char.chars().any(char::is_control) {
            this.settings.buffer_mut(field).push_str(key_char);
            cx.notify();
        }
    }
}
