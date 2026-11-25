// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page: the keys that dictate and read aloud from any app.
//!
//! On Linux the desktop binds them, through its `GlobalShortcuts` portal or
//! COSMIC's own settings: each shortcut's row says where it stands, adds it,
//! removes it where the desktop lets the daemon, and says where else to bind
//! it. On macOS the desktop binds nothing: the app bundle's listener
//! (`voice hotkey`) listens for the keys the daemon keeps, and the page
//! shows them, records new ones, goes back to the default, and says whether
//! the listener's background service is set to run. The logic is the core's
//! (`pages::shortcuts`).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, Keystroke, Modifiers, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::shortcuts::{
    self as page, ChangedIn, Effect, HotkeyKeys, Input, KeyPress, ListenerFix, Notice, Row,
};
use super_voice_app_core::state::shortcuts::ShortcutsState;
use super_voice_shared::models::hotkey;
use super_voice_shared::models::shortcuts::{Shortcut, ShortcutId, ShortcutList};

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand};
use crate::shared::Shared;
use crate::ui::card::Card;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::status::{Dot, StatusDot};
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, t_with};

/// The modifiers a Mac shows as symbols, each on its own key cap.
const MAC_MODIFIERS: [char; 4] = ['⌃', '⌥', '⇧', '⌘'];

pub struct ShortcutsPage {
    state: ShortcutsState,
    /// Whether this is the Mac's page: the listener's keys, not the
    /// desktop's shortcuts.
    mac: bool,
    /// Recording the next key press as the listener's keys: what the row
    /// says meanwhile.
    recording: Option<SharedString>,
    /// Takes the key presses while recording.
    recorder: FocusHandle,
}

impl PageView for ShortcutsPage {
    fn new(_: Page, _: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mac = cfg!(target_os = "macos");
        // Back from System Settings, where the listener may have been
        // switched on.
        if mac {
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() && this.state.listener.is_some() {
                    this.send(Input::RefreshListener, window, cx);
                }
            })
            .detach();
        }
        Self {
            state: ShortcutsState::default(),
            mac,
            recording: None,
            recorder: cx.focus_handle(),
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh(window, cx);
    }

    // Someone may have changed the shortcuts in the desktop's settings.
    fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh(window, cx);
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        if self.mac
            && let Some(input) = page::heard(event)
        {
            self.send(input, window, cx);
        }
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        if !self.mac || self.state.hotkey.is_none() {
            return Vec::new();
        }
        vec![
            PaletteCommand::new("change-keys", t("shortcut-change-keys"), Lucide::Keyboard)
                .detail(t("shortcut-dictate")),
        ]
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == "change-keys" {
            self.start_recording(window, cx);
        }
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        use super_voice_app_core::state::connection::AgentStatus;
        use super_voice_shared::models::shortcuts::{ShortcutMethod, ShortcutState};

        self.mac = scenes.contains(&"mac");
        if self.mac {
            self.state.hotkey = Some("ctrl+alt+Space".into());
            self.state.listener = Some(if scenes.contains(&"listener-off") {
                AgentStatus::RequiresApproval
            } else {
                AgentStatus::Enabled
            });
            if scenes.contains(&"recording") {
                self.start_recording(window, cx);
            }
            return;
        }
        let shortcut = |id, state, keys: &str, command: &str| Shortcut {
            id,
            state,
            keys: keys.into(),
            command: command.into(),
        };
        self.state.list = Some(ShortcutList {
            status: "success".into(),
            method: ShortcutMethod::Portal,
            removable: true,
            desktop: "KDE".into(),
            shortcuts: vec![
                shortcut(
                    ShortcutId::Dictate,
                    ShortcutState::Installed,
                    "Super+Space",
                    "voice record --write",
                ),
                shortcut(
                    ShortcutId::ReadAloud,
                    ShortcutState::Missing,
                    "Super+Shift+Space",
                    "voice speak --read",
                ),
            ],
        });
        if scenes.contains(&"adding") {
            self.state.changing = Some(ShortcutId::ReadAloud);
        }
    }
}

impl CorePage for ShortcutsPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        effects::run_effects(self, outcome.effects, window, cx);
        toast::notice::<Self>(
            window,
            cx,
            outcome.clear_notice,
            outcome.notice.map(|notice| {
                let (key, error) = match notice {
                    Notice::CouldNotLoad(e) => ("shortcuts-load-failed", e),
                    Notice::CouldNotChange(e) => ("shortcut-change-failed", e),
                    Notice::HotkeyRefused(e) => ("shortcut-refused", e),
                    Notice::CouldNotRegister(e) => ("shortcut-listener-register-failed", e),
                };
                Toast::error(t_with(key, &[("error", &error)]))
            }),
        );
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        Some(match effect {
            Effect::List => runner(page::run_list()),
            Effect::Add(id) => runner(page::run_add(id)),
            Effect::Remove(id) => runner(page::run_remove(id)),
            Effect::ReadHotkey => runner(page::run_read_hotkey()),
            Effect::SaveHotkey(binding) => runner(page::run_save_hotkey(binding)),
            Effect::ResetHotkey => runner(page::run_reset_hotkey()),
            // Blocking: `ServiceManagement` answers over XPC.
            Effect::ReadListener => runner(async {
                tokio::task::spawn_blocking(page::read_listener)
                    .await
                    .unwrap_or(Input::ListenerRead(
                        super_voice_app_core::state::connection::AgentStatus::NotInBundle,
                    ))
            }),
            Effect::RegisterListener => runner(async {
                tokio::task::spawn_blocking(page::register_listener)
                    .await
                    .unwrap_or_else(|e| Input::ListenerRegistered(Err(e.to_string())))
            }),
            Effect::OpenLoginItems => {
                page::open_login_items();
                return None;
            }
        })
    }
}

impl ShortcutsPage {
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = if self.mac {
            Input::RefreshListener
        } else {
            Input::Refresh
        };
        self.send(input, window, cx);
    }

    /// Record the next key press as the listener's keys.
    fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = Some(t("shortcut-press-keys"));
        self.recorder.focus(window, cx);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        cx.notify();
    }

    /// A key press while recording.
    fn record(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording.is_none() {
            return;
        }
        // Swallowed: a key pressed to record is not typed, nor a shortcut of
        // the app's.
        cx.stop_propagation();
        let Keystroke { modifiers, key, .. } = &event.keystroke;
        let held = modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform;
        // Esc alone cancels; with a modifier it can be the shortcut.
        if key == "escape" && !held {
            self.stop_recording(cx);
            return;
        }
        match page::key_press_named(listener_modifiers(*modifiers), key) {
            KeyPress::Keys { binding, .. } => {
                self.stop_recording(cx);
                self.send(Input::SetHotkey(binding), window, cx);
            }
            KeyPress::NeedsModifier => {
                self.recording = Some(t("shortcut-needs-modifier"));
                cx.notify();
            }
            KeyPress::Unusable => {
                self.recording = Some(t("shortcut-key-unusable"));
                cx.notify();
            }
        }
    }

    /// A press that sends `input`.
    fn press(
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _, window, cx| this.send(input.clone(), window, cx))
    }

    /// The desktop's shortcuts, one row each.
    fn desktop_rows(&self, list: &ShortcutList, cx: &mut Context<Self>) -> Vec<AnyElement> {
        list.shortcuts
            .iter()
            .enumerate()
            .flat_map(|(ix, shortcut)| {
                let separator = (ix > 0).then(|| Separator::horizontal().into_any_element());
                separator
                    .into_iter()
                    .chain([self.desktop_row(shortcut, list, cx).into_any_element()])
            })
            .collect()
    }

    /// One shortcut: what it does and where it stands, its keys once it is set
    /// up, and the button that adds or removes it, or copies the command to
    /// bind by hand.
    fn desktop_row(
        &self,
        shortcut: &Shortcut,
        list: &ShortcutList,
        cx: &mut Context<Self>,
    ) -> SettingsRow {
        let (title, does) = names(shortcut.id);
        let id = shortcut.id;
        let busy = self.state.changing == Some(id);
        let waiting = self.state.changing.is_some();
        let row = SettingsRow::new(title);
        match page::row(shortcut, list) {
            Row::Removable { keys, changed_in } => {
                let set_up = match changed_in {
                    ChangedIn::Cosmic => t("shortcut-set-up"),
                    ChangedIn::Gnome => t("shortcut-set-up-gnome"),
                    ChangedIn::Desktop => t("shortcut-set-up-desktop"),
                };
                row.description(format!("{does} {set_up}")).control(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(key_caps(&keys))
                        .child(
                            Button::new(element_id("remove", shortcut_key(id)))
                                .label(t("remove"))
                                .loading(busy)
                                .disabled(waiting)
                                .on_click(Self::press(Input::Remove(id), cx)),
                        ),
                )
            }
            // Through the portal elsewhere the daemon cannot remove it: the
            // desktop keeps it in its own settings.
            Row::Added { keys } => row
                .description(format!("{does} {}", t("shortcut-change-in-desktop")))
                .control(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(key_caps(&keys))
                        .child(
                            Button::new(element_id("added", shortcut_key(id)))
                                .label(t("added"))
                                .icon(IconName::Check)
                                .disabled(true),
                        ),
                ),
            Row::Missing { keys } => row
                .description(format!(
                    "{does} {}",
                    t_with("shortcut-missing", &[("keys", &keys)])
                ))
                .control(
                    Button::new(element_id("add", shortcut_key(id)))
                        .label(t("add-shortcut"))
                        .loading(busy)
                        .disabled(waiting)
                        .on_click(Self::press(Input::Add(id), cx)),
                ),
            Row::Taken { keys, command } => row
                .description(format!(
                    "{does} {}",
                    t_with("shortcut-taken", &[("keys", &keys), ("command", &command)])
                ))
                .control(copy_command(id, command)),
            Row::Elsewhere { command } => row
                .description(format!(
                    "{does} {}",
                    t_with("shortcut-elsewhere", &[("command", &command)])
                ))
                .control(copy_command(id, command)),
        }
    }

    /// Dictate, on a Mac: its keys, Change, which records the next key press,
    /// and Reset, back to ⌃⌥Space.
    fn hotkey_row(
        &self,
        keys: &HotkeyKeys,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> SettingsRow {
        let row =
            SettingsRow::new(t("shortcut-dictate")).description(t("shortcut-dictate-description"));
        let recording = self
            .recording
            .clone()
            .filter(|_| self.recorder.is_focused(window));
        let control = match recording {
            Some(hint) => h_flex()
                .id("recorder")
                .track_focus(&self.recorder)
                .on_key_down(cx.listener(Self::record))
                .gap_3()
                .items_center()
                .px_2()
                .py_1()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().ring)
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(hint),
                )
                .child(
                    Button::new("cancel-recording")
                        .small()
                        .ghost()
                        .label(t("cancel"))
                        .on_click(cx.listener(|this, _, _, cx| this.stop_recording(cx))),
                )
                .into_any_element(),
            None => h_flex()
                .gap_2()
                .items_center()
                // The handle stays in the tree, so a press of Change can
                // focus it before the row redraws.
                .child(div().track_focus(&self.recorder))
                .child(key_caps_mac(&keys.symbols))
                .child(
                    Button::new("change-keys")
                        .label(t("shortcut-change"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_recording(window, cx);
                        })),
                )
                .child(
                    Button::new("reset-keys")
                        .ghost()
                        .label(t("reset"))
                        .disabled(keys.is_default)
                        .tooltip(default_symbols())
                        .on_click(Self::press(Input::ResetHotkey, cx)),
                )
                .into_any_element(),
        };
        row.control(control)
    }

    /// Whether the listener's background service is set to run, and the one
    /// button that fixes it when it isn't.
    fn listener_row(fix: ListenerFix, cx: &mut Context<Self>) -> impl IntoElement {
        let (dot, words, button) = match fix {
            ListenerFix::None => (Dot::Green, t("shortcut-listener-running"), None),
            ListenerFix::Register => (
                Dot::Grey,
                t("shortcut-listener-not-registered"),
                Some(
                    Button::new("register")
                        .label(t("register"))
                        .on_click(Self::press(Input::RegisterListener, cx)),
                ),
            ),
            ListenerFix::TurnOn => (
                Dot::Grey,
                t("shortcut-listener-switched-off"),
                Some(
                    Button::new("login-items")
                        .label(t("open-login-items"))
                        .icon(IconName::ExternalLink)
                        .on_click(Self::press(Input::OpenLoginItems, cx)),
                ),
            ),
            ListenerFix::NotInBundle => (Dot::Grey, t("shortcut-listener-not-in-bundle"), None),
        };
        h_flex()
            .w_full()
            .py_3()
            .gap_3()
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(StatusDot::new(dot))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(words),
                    ),
            )
            .children(button)
    }

    fn body(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let card = Card::new().title(t("keyboard-shortcuts"));
        if self.mac {
            let Some(keys) = page::hotkey_keys(&self.state) else {
                return reading(card, cx);
            };
            let hotkey = self.hotkey_row(&keys, window, cx);
            return card
                .child(v_flex().child(hotkey).when_some(
                    page::listener_fix(&self.state),
                    |rows, fix| {
                        rows.child(Separator::horizontal())
                            .child(Self::listener_row(fix, cx))
                    },
                ))
                .into_any_element();
        }
        let Some(list) = self.state.list.clone() else {
            return reading(card, cx);
        };
        card.child(v_flex().children(self.desktop_rows(&list, cx)))
            .into_any_element()
    }
}

impl Render for ShortcutsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::Shortcuts).description(t("shortcuts-description")))
            .child(
                div()
                    .id("shortcuts")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(820.))
                            .px(padding)
                            .pt_2()
                            .pb_6()
                            .gap_4()
                            .child(self.body(window, cx)),
                    ),
            )
    }
}

/// A shortcut's name on the page, and what it does.
fn names(id: ShortcutId) -> (SharedString, SharedString) {
    match id {
        ShortcutId::Dictate => (t("shortcut-dictate"), t("shortcut-dictate-description")),
        ShortcutId::ReadAloud => (
            t("shortcut-read-aloud"),
            t("shortcut-read-aloud-description"),
        ),
    }
}

/// A name for a shortcut's elements that stays the same in every language.
const fn shortcut_key(id: ShortcutId) -> &'static str {
    match id {
        ShortcutId::Dictate => "dictate",
        ShortcutId::ReadAloud => "read-aloud",
    }
}

/// A button that copies the command to bind by hand.
fn copy_command(id: ShortcutId, command: String) -> Button {
    Button::new(element_id("copy", shortcut_key(id)))
        .icon(IconName::Copy)
        .label(t("copy-command"))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(command.clone()));
        })
}

/// The card while the daemon's answer is on its way.
fn reading(card: Card, cx: &App) -> AnyElement {
    card.child(
        h_flex()
            .gap_2()
            .items_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(Spinner::new().small())
            .child(t("about-daemon-reading")),
    )
    .into_any_element()
}

/// One key cap: the kit's `Kbd`, with its words as they are.
fn key_cap(key: &str) -> Kbd {
    Kbd::new(Keystroke {
        modifiers: Modifiers::default(),
        key: key.to_string(),
        key_char: None,
    })
}

/// Keys as people write them (`Super+Shift+Space`), a cap for each.
fn key_caps(keys: &str) -> impl IntoElement {
    h_flex().gap_1().children(
        keys.split('+')
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(key_cap),
    )
}

/// Keys as a Mac shows them (`⌃⌥Space`), a cap for each modifier and one for
/// the key.
fn key_caps_mac(symbols: &str) -> impl IntoElement {
    let key = symbols.trim_start_matches(MAC_MODIFIERS);
    let modifiers = symbols[..symbols.len() - key.len()].chars();
    h_flex()
        .gap_1()
        .children(modifiers.map(|symbol| key_cap(&symbol.to_string())))
        .when(!key.is_empty(), |caps| caps.child(key_cap(key)))
}

/// The default keys, as a Mac shows them: what Reset goes back to.
fn default_symbols() -> String {
    hotkey::Binding::default_binding().symbols()
}

/// The modifiers a key press holds, as the listener names them.
fn listener_modifiers(modifiers: Modifiers) -> hotkey::Modifiers {
    let mut held = hotkey::Modifiers::NONE;
    for (on, modifier) in [
        (modifiers.control, hotkey::Modifiers::CONTROL),
        (modifiers.alt, hotkey::Modifiers::OPTION),
        (modifiers.shift, hotkey::Modifiers::SHIFT),
        (modifiers.platform, hotkey::Modifiers::COMMAND),
    ] {
        if on {
            held |= modifier;
        }
    }
    held
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_held_modifier_is_the_listeners() {
        let held = super::listener_modifiers(gpui_kit::Modifiers {
            control: true,
            platform: true,
            ..Default::default()
        });
        assert!(held.contains(super::hotkey::Modifiers::CONTROL));
        assert!(held.contains(super::hotkey::Modifiers::COMMAND));
        assert!(!held.contains(super::hotkey::Modifiers::SHIFT));
    }

    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "about-daemon-reading",
            "add-shortcut",
            "added",
            "cancel",
            "copy-command",
            "keyboard-shortcuts",
            "open-login-items",
            "register",
            "remove",
            "reset",
            "shortcut-change",
            "shortcut-change-failed",
            "shortcut-change-in-desktop",
            "shortcut-change-keys",
            "shortcut-dictate",
            "shortcut-dictate-description",
            "shortcut-elsewhere",
            "shortcut-key-unusable",
            "shortcut-listener-not-in-bundle",
            "shortcut-listener-not-registered",
            "shortcut-listener-register-failed",
            "shortcut-listener-running",
            "shortcut-listener-switched-off",
            "shortcut-missing",
            "shortcut-needs-modifier",
            "shortcut-press-keys",
            "shortcut-read-aloud",
            "shortcut-read-aloud-description",
            "shortcut-refused",
            "shortcut-set-up",
            "shortcut-set-up-desktop",
            "shortcut-set-up-gnome",
            "shortcut-taken",
            "shortcuts-description",
            "shortcuts-load-failed",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}

/// An element's id from two names: `remove-dictate`.
fn element_id(prefix: &str, name: &str) -> gpui_kit::ElementId {
    gpui_kit::ElementId::Name(format!("{prefix}-{name}").into())
}
