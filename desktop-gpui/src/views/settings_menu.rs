//! The account settings popover — the port of
//! `frontend/src/components/SettingsMenu.tsx`, including its hand-rolled text
//! inputs (gpui has no text field; echo's pattern: a focusable div, a String buffer,
//! and key handling done by hand).
//!
//! Two accounts live here, rendered by one `account_section`: the OpenF1 account for
//! live data (OpenF1 issues one-hour tokens, so the panel asks for the account rather
//! than a token; the backend exchanges it and re-exchanges on expiry) and the Formula 1
//! account FastF1 signs in with to download historical timing, which needs an F1 TV
//! subscription linked to that account. Password inputs are never repopulated from the
//! saved state; the backend only ever reports the username.

use gpui::{Context, Div, FocusHandle, SharedString, Window, div, prelude::*, px, rems, svg};
use interval_desktop_core::replay_quality::ChannelBadge;
use interval_desktop_core::settings_panel::{
    self, F1LoginProbe, F1LoginSettings, OpenF1AuthSource, OpenF1LoginProbe, OpenF1LoginSettings,
    is_submittable_login,
};

use super::ui;
use crate::{IntervalApp, theme, updates::UpdateState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Account {
    OpenF1,
    F1,
}

impl Account {
    fn index(self) -> usize {
        match self {
            Account::OpenF1 => 0,
            Account::F1 => 1,
        }
    }
}

pub struct AccountUi {
    pub username_input: String,
    pub password_input: String,
    pub reveal: bool,
    pub error: Option<String>,
    pub username_focus: FocusHandle,
    pub password_focus: FocusHandle,
}

impl AccountUi {
    fn new(cx: &mut Context<IntervalApp>) -> Self {
        Self {
            username_input: String::new(),
            password_input: String::new(),
            reveal: false,
            error: None,
            username_focus: cx.focus_handle(),
            password_focus: cx.focus_handle(),
        }
    }

    pub fn clear_inputs(&mut self) {
        self.username_input.clear();
        self.password_input.clear();
    }
}

/// UI state for the popover. The OpenF1 settings are `None` until the initial fetch
/// settles, and `Some(None)` when the routes are unavailable (a web deployment), which
/// hides the gear entirely.
pub struct SettingsUi {
    pub open: bool,
    pub busy: bool,
    pub openf1: AccountUi,
    pub f1: AccountUi,
    pub probe: Option<OpenF1LoginProbe>,
    pub f1_probe: Option<F1LoginProbe>,
    pub settings: Option<Option<OpenF1LoginSettings>>,
    pub f1_settings: Option<F1LoginSettings>,
}

impl SettingsUi {
    pub fn new(cx: &mut Context<IntervalApp>) -> Self {
        Self {
            open: false,
            busy: false,
            openf1: AccountUi::new(cx),
            f1: AccountUi::new(cx),
            probe: None,
            f1_probe: None,
            settings: None,
            f1_settings: None,
        }
    }

    pub fn account(&self, account: Account) -> &AccountUi {
        match account {
            Account::OpenF1 => &self.openf1,
            Account::F1 => &self.f1,
        }
    }

    pub fn account_mut(&mut self, account: Account) -> &mut AccountUi {
        match account {
            Account::OpenF1 => &mut self.openf1,
            Account::F1 => &mut self.f1,
        }
    }

    pub fn can_submit(&self, account: Account) -> bool {
        let inputs = self.account(account);
        !self.busy && is_submittable_login(&inputs.username_input, &inputs.password_input)
    }

    pub fn begin(&mut self, account: Account) {
        self.busy = true;
        self.account_mut(account).error = None;
        match account {
            Account::OpenF1 => self.probe = None,
            Account::F1 => self.f1_probe = None,
        }
    }

    fn buffer_mut(&mut self, field: Field) -> &mut String {
        let inputs = self.account_mut(field.account);
        match field.half {
            Half::Username => &mut inputs.username_input,
            Half::Password => &mut inputs.password_input,
        }
    }

    fn focus_handle(&self, field: Field) -> &FocusHandle {
        let inputs = self.account(field.account);
        match field.half {
            Half::Username => &inputs.username_focus,
            Half::Password => &inputs.password_focus,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Half {
    Username,
    Password,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Field {
    account: Account,
    half: Half,
}

impl Field {
    fn other(self) -> Self {
        let half = match self.half {
            Half::Username => Half::Password,
            Half::Password => Half::Username,
        };
        Field { half, ..self }
    }
}

/// The chord every platform's user reaches for to paste; a Mac that only matched
/// `control` would make the inputs impossible to paste into.
fn is_paste_chord(modifiers: &gpui::Modifiers) -> bool {
    modifiers.secondary() || modifiers.control
}

struct AccountSection {
    account: Account,
    title: &'static str,
    source_line: String,
    status_line: Option<String>,
    username_placeholder: &'static str,
    can_test: bool,
    can_clear: bool,
    badge: Option<ChannelBadge>,
    probe_message: Option<String>,
    notice: Option<&'static str>,
    hint: &'static str,
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

    let openf1_section = AccountSection {
        account: Account::OpenF1,
        title: "OPENF1 ACCOUNT (LIVE)",
        source_line: settings_panel::login_source_line(&current),
        status_line: None,
        username_placeholder: "OpenF1 account email",
        can_test: !busy && current.configured,
        can_clear: !busy && current.source == OpenF1AuthSource::Settings,
        badge: app.settings.probe.as_ref().map(settings_panel::probe_badge),
        probe_message: app
            .settings
            .probe
            .as_ref()
            .map(|probe| probe.message.clone()),
        notice: settings_panel::env_override_notice(&current),
        hint: "OpenF1 tokens last an hour; the app signs in again on its own.",
    };
    let f1_section = app.settings.f1_settings.as_ref().map(|f1| AccountSection {
        account: Account::F1,
        title: "F1 TV ACCOUNT (HISTORICAL)",
        source_line: settings_panel::f1_login_source_line(f1),
        status_line: settings_panel::f1_verification_line(f1),
        username_placeholder: "Formula 1 account email",
        can_test: !busy && f1.configured,
        can_clear: !busy && f1.source == OpenF1AuthSource::Settings,
        badge: app
            .settings
            .f1_probe
            .as_ref()
            .map(settings_panel::f1_probe_badge),
        probe_message: app
            .settings
            .f1_probe
            .as_ref()
            .map(|probe| probe.message.clone()),
        notice: settings_panel::f1_env_override_notice(f1),
        hint: settings_panel::F1_ACCOUNT_HINT,
    });
    let saved_usernames = [
        current.username.clone(),
        app.settings
            .f1_settings
            .as_ref()
            .and_then(|f1| f1.username.clone()),
    ];

    let (update_label, update_hint, update_active) = app.update_state.presentation();
    let update_available = matches!(app.update_state, UpdateState::Available(_));
    let version_line = match interval_desktop_core::update::current_version() {
        Some(version) => format!("Version {version}"),
        None => "Development build".to_string(),
    };
    let update_section = section_frame("APP")
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
    let stored_line = current.path.clone().map(|path| {
        div()
            .mt_2()
            .text_size(rems(0.62))
            .text_color(ui::faint())
            .child(format!("Stored in {path}"))
    });

    let max_height = window.viewport_size().height - px(96.0);
    let sections = std::iter::once(openf1_section)
        .chain(f1_section)
        .map(|section| {
            let saved = saved_usernames[section.account.index()].clone();
            account_section(app, window, cx, section, saved).into_any_element()
        })
        .collect::<Vec<_>>();

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
                            window.focus(&this.settings.openf1.username_focus, cx);
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
                    gpui::anchored().snap_to_window_with_margin(px(8.0)).child(
                        div()
                            .id("settings-panel")
                            .occlude()
                            .mt(px(4.0))
                            .w(rems(25.0))
                            .max_h(max_height)
                            .overflow_y_scroll()
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
                            .children(sections)
                            .child(update_section)
                            .children(stored_line),
                    ),
                ))
            }),
    )
}

fn section_frame(title: &'static str) -> Div {
    div()
        .mt_2()
        .pt_2()
        .border_t_1()
        .border_color(theme::LINE())
        .child(
            div()
                .mb_2()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::ACCENT())
                .child(title),
        )
}

fn account_section(
    app: &IntervalApp,
    window: &Window,
    cx: &mut Context<IntervalApp>,
    section: AccountSection,
    saved_username: Option<String>,
) -> Div {
    let account = section.account;
    let inputs = app.settings.account(account);
    let reveal = inputs.reveal;
    let error = inputs.error.clone();
    let can_save = app.settings.can_submit(account);

    let username_placeholder: Option<SharedString> = inputs.username_input.is_empty().then(|| {
        match saved_username.as_deref().filter(|name| !name.is_empty()) {
            Some(name) => SharedString::from(name.to_string()),
            None => SharedString::from(section.username_placeholder),
        }
    });
    let password_placeholder: Option<SharedString> = inputs
        .password_input
        .is_empty()
        .then(|| SharedString::from("Password"));
    let password_display: SharedString = if reveal {
        SharedString::from(inputs.password_input.clone())
    } else {
        SharedString::from("•".repeat(inputs.password_input.chars().count()))
    };
    let username_display = SharedString::from(inputs.username_input.clone());

    let username_field = text_input(
        app,
        window,
        cx,
        Field {
            account,
            half: Half::Username,
        },
        username_display,
        username_placeholder,
    );
    let password_field = text_input(
        app,
        window,
        cx,
        Field {
            account,
            half: Half::Password,
        },
        password_display,
        password_placeholder,
    );

    let frame = match account {
        Account::OpenF1 => div().child(
            div()
                .mb_2()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::ACCENT())
                .child(section.title),
        ),
        Account::F1 => section_frame(section.title),
    };

    frame
        .child(
            div()
                .mb_2()
                .text_color(ui::muted())
                .child(section.source_line),
        )
        .children(section.status_line.map(|line| {
            div()
                .mb_2()
                .text_size(rems(0.62))
                .text_color(ui::muted())
                .child(line)
        }))
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
                        .id(("settings-reveal", account.index()))
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
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            let inputs = this.settings.account_mut(account);
                            inputs.reveal = !inputs.reveal;
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
                    ("settings-save", account.index()),
                    "SIGN IN".into(),
                    can_save,
                    true,
                    cx,
                    move |this, cx| this.settings_save(account, cx),
                ))
                .child(action_button(
                    ("settings-test", account.index()),
                    "TEST".into(),
                    section.can_test,
                    false,
                    cx,
                    move |this, cx| this.settings_test(account, cx),
                ))
                .child(action_button(
                    ("settings-clear", account.index()),
                    "SIGN OUT".into(),
                    section.can_clear,
                    false,
                    cx,
                    move |this, cx| this.settings_clear(account, cx),
                ))
                .children(section.badge.as_ref().map(|badge| ui::channel_badge(badge))),
        )
        .children(
            section
                .probe_message
                .map(|message| div().mb_2().text_color(ui::muted()).child(message)),
        )
        .children(error.map(|message| div().mb_2().text_color(theme::DANGER()).child(message)))
        .children(
            section
                .notice
                .map(|notice| div().mb_2().text_color(theme::AMBER()).child(notice)),
        )
        .child(
            div()
                .mb_1()
                .text_size(rems(0.62))
                .text_color(ui::faint())
                .child(section.hint),
        )
}

fn action_button(
    id: impl Into<gpui::ElementId>,
    label: SharedString,
    enabled: bool,
    accent: bool,
    cx: &mut Context<IntervalApp>,
    on_click: impl Fn(&mut IntervalApp, &mut Context<IntervalApp>) + 'static,
) -> gpui::Stateful<Div> {
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
                        style
                            .border_color(theme::ACCENT())
                            .text_color(theme::ACCENT())
                    })
            }
        })
        .when(enabled, |el| {
            el.on_click(cx.listener(move |this, _, _window, cx| on_click(this, cx)))
        })
        .child(label)
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
    let id: gpui::ElementId = match field.half {
        Half::Username => ("settings-username-input", field.account.index()).into(),
        Half::Password => ("settings-password-input", field.account.index()).into(),
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
        .on_key_down(
            cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                handle_input_key(this, field, event, window, cx);
            }),
        )
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
        // Two fields per account, so tab and shift-tab both mean "the other one".
        "tab" => {
            window.focus(this.settings.focus_handle(field.other()), cx);
            cx.notify();
            return;
        }
        "enter" => {
            if this.settings.can_submit(field.account) {
                this.settings_save(field.account, cx);
            } else if field.half == Half::Username {
                // Enter on a filled email is "next field", as in a browser form.
                window.focus(this.settings.focus_handle(field.other()), cx);
                cx.notify();
            }
            return;
        }
        "v" if is_paste_chord(&keystroke.modifiers) => {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                // A pasted email never wants its surrounding whitespace; a pasted
                // password is taken as-is, since OpenF1 generated it.
                let text = match field.half {
                    Half::Username => text.trim().to_string(),
                    Half::Password => text,
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
