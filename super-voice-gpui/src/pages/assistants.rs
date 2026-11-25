// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page: the AI assistants on this computer, connected to
//! Super Voice or not, and keys for any other assistant that supports MCP.
//!
//! Each assistant the daemon knows has a row: Connect… asks first, in a
//! dialog that says what the assistant will be able to do; a connected one
//! shows its switches and its open sessions under its row, and Disconnect…
//! asks before it takes the assistant's access away. Another assistant gets
//! a key made by hand, shown once in a dialog with the address to put beside
//! it; Remove… asks before the key stops working.
//!
//! The logic is the core's (`pages::assistants`), which holds the Connect
//! and key dialogs' state; this view opens and closes the kit's dialogs to
//! match it. Sessions start, speak, listen and end on their own, so the list
//! is read again every two seconds while the page is on screen.

use std::cell::Cell;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input as TextInput, InputEvent, InputState};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, px,
};
use super_voice_app_core::pages::assistants::{
    self as page, Ago, Effect, Flag, Input, MadeOn, Notice, folder_name, shown_folder,
};
use super_voice_app_core::state::assistants::{AssistantsState, ConnectDialog, KeyDialog};
use super_voice_shared::models::assistants::{
    Assistant, AssistantId, AssistantSession, AssistantState, CustomKey, NewKey,
};

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand};
use crate::shared::Shared;
use crate::ui::card::Card;
use crate::ui::code_line;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::status::{Dot, StatusPill};
use crate::ui::toast::{self, Toast};
use crate::ui::width::{self, WindowSize};
use crate::{t, t_with};

/// How often the list is read again while the page is on screen.
const POLL: Duration = Duration::from_secs(2);
/// A page not drawn for this long is off screen: the reading stops.
const UNSEEN: Duration = Duration::from_secs(3);
/// How far a connected assistant's switches and sessions sit in, under its
/// name.
const INSET: f32 = 44.;
/// The dialogs' width, when the window leaves room for it.
const DIALOG_WIDTH: f32 = 460.;

/// Which of the core's dialogs is open over the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Connect,
    Key,
}

pub struct AssistantsPage {
    state: AssistantsState,
    shared: Shared,
    /// The key's name, in the key dialog.
    key_name: Entity<InputState>,
    /// The dialog this view has open, to match the core's.
    open: Option<Open>,
    /// What failed, while a dialog is open: said in the dialog, beside what
    /// failed, rather than in a toast behind it.
    dialog_error: Option<SharedString>,
    /// When the page was last drawn, for the reading every two seconds.
    drawn: Cell<Instant>,
    poll: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PageView for AssistantsPage {
    fn new(_: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_name = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![cx.subscribe_in(
            &key_name,
            window,
            |this, state, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let name = state.read(cx).value().to_string();
                    this.send(Input::KeyNameChanged(name), window, cx);
                }
                InputEvent::PressEnter { .. } => this.send(Input::CreateKey, window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        )];
        Self {
            state: AssistantsState::default(),
            shared: shared.clone(),
            key_name,
            open: None,
            dialog_error: None,
            drawn: Cell::new(Instant::now()),
            poll: None,
            _subscriptions: subscriptions,
        }
    }

    // The daemon reads each assistant's config on every request, and an
    // assistant may have been installed, or its config edited by hand.
    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(Input::Refresh, window, cx);
    }

    fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drawn.set(Instant::now());
        if !self.shared.connection.read(cx).is_down() {
            self.send(Input::Refresh, window, cx);
        }
        self.keep_reading(window, cx);
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        if self.state.list.is_none() {
            return Vec::new();
        }
        vec![
            PaletteCommand::new("create-key", t("create-key"), Lucide::KeyRound)
                .detail(t("another-assistant"))
                .keywords([t("any-mcp-assistant")]),
        ]
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == "create-key" {
            self.send(Input::AskKey, window, cx);
        }
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        self.state.list = Some(pretend_list());
        if scenes.contains(&"connect") {
            self.send(Input::AskConnect(AssistantId::OpenCode), window, cx);
        } else if scenes.contains(&"key-made") {
            self.send(Input::AskKey, window, cx);
            self.send(
                Input::KeyCreated(Ok(NewKey {
                    id: "k2".into(),
                    name: "Zed".into(),
                    key: "sv_live_7f3a9c2e81b44d0f9a6c5e1b2d3f4a5b".into(),
                    address: "http://127.0.0.1:7300/v1/mcp".into(),
                })),
                window,
                cx,
            );
        }
    }
}

/// The made-up list: Claude Code connected with one session listening, Codex
/// not found, `OpenCode` found, and one key made by hand.
#[cfg(feature = "screenshot")]
fn pretend_list() -> super_voice_shared::models::assistants::AssistantList {
    use super_voice_shared::models::assistants::AssistantList;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/me".into());
    let assistant = |id, state| Assistant {
        id,
        state,
        announce_when_needed: true,
        conversation_context: true,
        show_talk: true,
        autosend_dictated_prompts: false,
        sessions: Vec::new(),
    };
    let mut claude = assistant(AssistantId::ClaudeCode, AssistantState::Connected);
    claude.sessions = vec![AssistantSession {
        id: Some("s1".into()),
        folder: Some(format!("{home}/super-libre/super-voice")),
        agents: 1,
        speaking: false,
        listening: true,
        started_at: "2026-11-25T09:15:00Z".into(),
        last_used_at: None,
    }];
    AssistantList {
        assistants: vec![
            claude,
            assistant(AssistantId::Codex, AssistantState::NotFound),
            assistant(AssistantId::OpenCode, AssistantState::Found),
            assistant(AssistantId::Antigravity, AssistantState::NotFound),
        ],
        custom_keys: vec![CustomKey {
            id: "k1".into(),
            name: "My editor".into(),
            created_at: "2026-11-16T11:03:00Z".into(),
            sessions: Vec::new(),
        }],
        address: "http://127.0.0.1:7300/v1/mcp".into(),
    }
}

impl CorePage for AssistantsPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        if outcome.clear_notice {
            self.dialog_error = None;
        }
        // The dialogs first: one that opens takes the focus, and the key's
        // name field takes it from the dialog.
        self.match_dialogs(window, cx);
        effects::run_effects(self, outcome.effects, window, cx);
        let notice = outcome.notice.map(notice_words);
        match notice {
            Some(words) if self.state.dialog_open() => {
                self.dialog_error = Some(words);
                toast::clear::<Self>(window, cx);
            }
            notice => {
                toast::notice::<Self>(window, cx, outcome.clear_notice, notice.map(Toast::error));
            }
        }
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        Some(match effect {
            Effect::List => runner(page::run_list()),
            Effect::Connect(id) => runner(page::run_connect(id)),
            Effect::Disconnect(id) => runner(page::run_disconnect(id)),
            Effect::SetFlag(id, flag, on) => runner(page::run_set_flag(id, flag, on)),
            Effect::CreateKey(name) => runner(page::run_create_key(name)),
            Effect::RemoveKey(id) => runner(page::run_remove_key(id)),
            Effect::FocusKeyName => {
                let field = self.key_name.clone();
                window.defer(cx, move |window, cx| {
                    field.update(cx, |field, cx| {
                        field.set_value("", window, cx);
                        field.focus(window, cx);
                    });
                });
                return None;
            }
        })
    }
}

impl AssistantsPage {
    /// Read the list again every two seconds while the page is on screen
    /// and the daemon answers.
    fn keep_reading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.poll.is_some() {
            return;
        }
        self.poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let reading = this.update_in(cx, |page, window, cx| {
                    if page.drawn.get().elapsed() > UNSEEN {
                        page.poll = None;
                        return false;
                    }
                    if !page.shared.connection.read(cx).is_down() {
                        page.send(Input::Refresh, window, cx);
                    }
                    true
                });
                if !matches!(reading, Ok(true)) {
                    return;
                }
            }
        }));
    }

    /// Open or close the kit's dialogs to match the core's: once the window
    /// is free to, since this may run from inside a dialog's own handler.
    fn match_dialogs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = if self.state.connect.is_some() {
            Some(Open::Connect)
        } else if self.state.key.is_some() {
            Some(Open::Key)
        } else {
            None
        };
        if wanted == self.open {
            return;
        }
        let close = self.open.is_some();
        self.open = wanted;
        let this = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            if close {
                window.close_dialog(cx);
            }
            match wanted {
                Some(Open::Connect) => window.open_dialog(cx, connect_dialog(this)),
                Some(Open::Key) => window.open_dialog(cx, key_dialog(this)),
                None => {}
            }
        });
    }

    /// A press that sends `input`.
    fn press(
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _, window, cx| this.send(input.clone(), window, cx))
    }

    /// Ask before `input`, which takes something away: the kit's alert
    /// dialog, naming the thing and the verb.
    fn confirm(
        title: SharedString,
        body: SharedString,
        verb: SharedString,
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        let this = cx.weak_entity();
        move |_, window, cx| {
            let this = this.clone();
            let (title, body, verb, input) =
                (title.clone(), body.clone(), verb.clone(), input.clone());
            window.open_alert_dialog(cx, move |alert, _, _| {
                let this = this.clone();
                let input = input.clone();
                alert
                    .title(title.clone())
                    .description(body.clone())
                    .confirm()
                    .ok_text(verb.clone())
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text(t("cancel"))
                    .on_ok(move |_, window, cx| {
                        _ = this.update(cx, |page, cx| page.send(input.clone(), window, cx));
                        true
                    })
            });
        }
    }

    /// The assistants on this computer.
    fn on_this_computer(&self, inset: bool, cx: &mut Context<Self>) -> Option<Card> {
        let list = self.state.list.as_ref()?;
        let rows: Vec<AnyElement> = AssistantId::ALL
            .into_iter()
            .filter_map(|id| list.assistants.iter().find(|a| a.id == id))
            .enumerate()
            .flat_map(|(ix, assistant)| {
                let separator = (ix > 0).then(|| Separator::horizontal().into_any_element());
                separator
                    .into_iter()
                    .chain([self.assistant(assistant, inset, cx)])
            })
            .collect();
        Some(
            Card::new()
                .title(t("on-this-computer"))
                .child(v_flex().children(rows)),
        )
    }

    /// One assistant: its name and what it does or where it stands, and
    /// Connect… or Disconnect…; under a connected one, its switches and
    /// sessions.
    fn assistant(&self, assistant: &Assistant, inset: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = assistant.id;
        let name = id.display_name();
        let connected = assistant.state == AssistantState::Connected;
        let mut about = match assistant.state {
            AssistantState::NotFound => t("assistant-not-found").to_string(),
            _ => does(id).to_string(),
        };
        // An assistant reads its config when it starts.
        if connected && self.state.connected_here.contains(&id) {
            about.push(' ');
            about.push_str(&t_with("assistant-restart-to-use", &[("name", name)]));
        }
        let control = if connected {
            let disconnecting = self.state.disconnecting.contains(&id);
            Button::new(element_id("disconnect", assistant_key(id)))
                .label(t("disconnect-ellipsis"))
                .loading(disconnecting)
                .disabled(disconnecting)
                .on_click(Self::confirm(
                    t_with("disconnect-assistant-question", &[("name", name)]),
                    t_with("disconnect-assistant-description", &[("name", name)]),
                    t("disconnect"),
                    Input::Disconnect(id),
                    cx,
                ))
        } else {
            Button::new(element_id("connect", assistant_key(id)))
                .label(t("connect-ellipsis"))
                .disabled(assistant.state == AssistantState::NotFound)
                .on_click(Self::press(Input::AskConnect(id), cx))
        };
        let title = h_flex()
            .gap_2()
            .items_center()
            .child(name)
            .when(connected, |title| {
                title.child(StatusPill::new(Dot::Green, t("assistant-connected")))
            });
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_start()
                    .child(tile(cx))
                    .child(
                        div().flex_1().min_w_0().child(
                            SettingsRow::new(name)
                                .description(about)
                                .control(control)
                                .heading(title),
                        ),
                    ),
            )
            .when(connected, |row| {
                row.child(
                    v_flex()
                        .when(inset, |details| details.pl(px(INSET)))
                        .pb_3()
                        .child(Self::switches(assistant, cx))
                        .child(Self::sessions(&assistant.sessions, cx)),
                )
            })
            .into_any_element()
    }

    /// A connected assistant's switches, with what each does.
    fn switches(assistant: &Assistant, cx: &mut Context<Self>) -> impl IntoElement {
        let id = assistant.id;
        let name = id.display_name();
        v_flex().children(page::flags(id).into_iter().map(|flag| {
            let (title, description) = match flag {
                Flag::ShowTalk => (
                    t("show-talk-above-prompt"),
                    t("show-talk-above-prompt-description"),
                ),
                Flag::AutosendDictatedPrompts => (
                    t("autosend-dictated-prompts"),
                    t("autosend-dictated-prompts-description"),
                ),
                Flag::AnnounceWhenNeeded => (
                    t("announce-when-needed"),
                    t_with("announce-when-needed-description", &[("name", name)]),
                ),
                Flag::ConversationContext => (
                    t("use-conversation-as-context"),
                    t("use-conversation-as-context-description"),
                ),
            };
            SettingsRow::new(title.clone())
                .description(description)
                .inline()
                .control(
                    Switch::new(element_id(assistant_key(id), flag_key(flag)))
                        .checked(page::flag_on(assistant, flag))
                        .accessibility_label(title)
                        .on_change(cx.listener(move |this, on: &bool, window, cx| {
                            this.send(Input::SetFlag(id, flag, *on), window, cx);
                        })),
                )
        }))
    }

    /// The open sessions, in a bordered list under their heading.
    fn sessions(sessions: &[AssistantSession], cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        v_flex()
            .gap_1p5()
            .pt_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t_with(
                        "sessions-open",
                        &[("count", &sessions.len().to_string())],
                    )),
            )
            .when(!sessions.is_empty(), |list| {
                list.child(
                    v_flex()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .children(sessions.iter().enumerate().map(|(ix, session)| {
                            session_row(session, home.as_deref(), cx)
                                .when(ix > 0, |row| row.border_t_1().border_color(theme.border))
                        })),
                )
            })
    }

    /// Any other assistant: Create key…, and the keys made so far with their
    /// sessions.
    fn another_assistant(&self, inset: bool, cx: &mut Context<Self>) -> Option<Card> {
        let list = self.state.list.as_ref()?;
        let mut rows = vec![
            SettingsRow::new(t("any-mcp-assistant"))
                .description(t("any-mcp-assistant-description"))
                .control(
                    Button::new("create-key")
                        .icon(Lucide::KeyRound)
                        .label(t("create-key-ellipsis"))
                        .on_click(Self::press(Input::AskKey, cx)),
                )
                .into_any_element(),
        ];
        for key in &list.custom_keys {
            rows.push(Separator::horizontal().into_any_element());
            rows.push(Self::key_row(key, inset, cx));
        }
        Some(
            Card::new()
                .title(t("another-assistant"))
                .child(v_flex().children(rows)),
        )
    }

    /// A key made by hand: its name, when it was made, Remove…, and its
    /// sessions.
    fn key_row(key: &CustomKey, inset: bool, cx: &mut Context<Self>) -> AnyElement {
        let made = match page::made_on_now(&key.created_at) {
            MadeOn::Today(time) => t_with("today-at", &[("time", &time)]),
            MadeOn::Yesterday(time) => t_with("yesterday-at", &[("time", &time)]),
            MadeOn::Earlier(when) | MadeOn::Unread(when) => when.into(),
        };
        let row = SettingsRow::new(key.name.clone())
            .description(made)
            .control(
                Button::new(SharedString::from(format!("remove-key-{}", key.id)))
                    .label(t("remove-ellipsis"))
                    .on_click(Self::confirm(
                        t_with("remove-key-question", &[("name", &key.name)]),
                        t("remove-key-description"),
                        t("remove"),
                        Input::RemoveKey(key.id.clone()),
                        cx,
                    )),
            );
        v_flex()
            .child(row)
            .when(!key.sessions.is_empty(), |row| {
                row.child(
                    div()
                        .when(inset, |sessions| sessions.pl(px(INSET)))
                        .pb_3()
                        .child(Self::sessions(&key.sessions, cx)),
                )
            })
            .into_any_element()
    }
}

impl Render for AssistantsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drawn.set(Instant::now());
        let padding = width::page_padding(window);
        let inset = WindowSize::of(window) != WindowSize::Narrow;
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::Assistants).description(t("assistants-intro")))
            .child(
                div()
                    .id("assistants")
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
                            .children(self.on_this_computer(inset, cx))
                            .children(self.another_assistant(inset, cx))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t("assistants-intro-caption")),
                            ),
                    ),
            )
    }
}

/// The Connect dialog, drawn from the core's state each time it is drawn:
/// what the assistant will be able to do, how it is connected, and Connect,
/// which waits for the daemon (Claude Code's plugin can take a minute).
fn connect_dialog(
    this: WeakEntity<AssistantsPage>,
) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static {
    move |dialog, window, cx| {
        let Some(page) = this.upgrade() else {
            return dialog;
        };
        let page = page.read(cx);
        let Some(ConnectDialog { id, connecting }) = page.state.connect else {
            return dialog;
        };
        let error = page.dialog_error.clone();
        let name = id.display_name();
        let how = if page::connects_with_a_plugin(id) {
            t_with("connect-plugin-caption", &[("name", name)])
        } else {
            t("connect-codex-caption")
        };
        let send = |input: Input| {
            let this = this.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                _ = this.update(cx, |page, cx| page.send(input.clone(), window, cx));
            }
        };
        let cancel = this.clone();
        dialog
            .w(dialog_width(window))
            .close_button(false)
            .title(t_with("connect-assistant-question", &[("name", name)]))
            .child(
                v_flex()
                    .gap_3()
                    .text_sm()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(t_with("assistant-will-be-able-to", &[("name", name)]))
                            .child(t("assistant-can-speak"))
                            .child(t("assistant-can-listen")),
                    )
                    .child(div().text_color(cx.theme().muted_foreground).child(how))
                    .when_some(error, |body, error| {
                        body.child(Alert::error("connect-error", error))
                    }),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel-connect")
                            .label(t("cancel"))
                            .disabled(connecting)
                            .on_click(send(Input::CancelConnect)),
                    )
                    .child(
                        Button::new("connect")
                            .primary()
                            .label(if connecting {
                                t("connecting-assistant")
                            } else {
                                t("connect")
                            })
                            .loading(connecting)
                            .on_click(send(Input::Connect)),
                    ),
            )
            // Esc: the core closes it, unless it is connecting.
            .on_cancel(move |_, window, cx| {
                _ = cancel.update(cx, |page, cx| page.send(Input::CancelConnect, window, cx));
                false
            })
    }
}

/// The key dialog, drawn from the core's state: the key's name and Create,
/// then the address and the key, each to copy, shown this once.
fn key_dialog(
    this: WeakEntity<AssistantsPage>,
) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static {
    move |dialog, window, cx| {
        let Some(page) = this.upgrade() else {
            return dialog;
        };
        let page = page.read(cx);
        let Some(key) = page.state.key.clone() else {
            return dialog;
        };
        let error = page.dialog_error.clone();
        let field = page.key_name.clone();
        let send = |input: Input| {
            let this = this.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                _ = this.update(cx, |page, cx| page.send(input.clone(), window, cx));
            }
        };
        let close = this.clone();
        let dialog = dialog
            .w(dialog_width(window))
            .close_button(false)
            .title(t("key-for-another-assistant"))
            .on_cancel(move |_, window, cx| {
                _ = close.update(cx, |page, cx| page.send(Input::CloseKey, window, cx));
                false
            });
        let muted = cx.theme().muted_foreground;
        match key {
            KeyDialog::Naming { creating, .. } => {
                let can_create = key.name_to_create().is_some();
                dialog
                    .child(
                        v_flex()
                            .gap_1p5()
                            .text_sm()
                            .child(div().font_weight(FontWeight::MEDIUM).child(t("name")))
                            .child(TextInput::new(&field).disabled(creating))
                            .child(div().text_color(muted).child(t("key-name-caption")))
                            .when_some(error, |body, error| {
                                body.child(Alert::error("key-error", error))
                            }),
                    )
                    .footer(
                        DialogFooter::new()
                            .child(
                                Button::new("cancel-key")
                                    .label(t("cancel"))
                                    .disabled(creating)
                                    .on_click(send(Input::CloseKey)),
                            )
                            .child(
                                Button::new("create-key")
                                    .primary()
                                    .label(if creating {
                                        t("creating-key")
                                    } else {
                                        t("create")
                                    })
                                    .loading(creating)
                                    .disabled(!can_create && !creating)
                                    .on_click(send(Input::CreateKey)),
                            ),
                    )
            }
            KeyDialog::Made(made) => dialog.child(made_key(&made, cx)).footer(
                DialogFooter::new().child(
                    Button::new("done-key")
                        .primary()
                        .label(t("done"))
                        .on_click(send(Input::CloseKey)),
                ),
            ),
        }
    }
}

/// The key just made: its name, then the address and the key, each with
/// Copy, and that it is shown only now.
fn made_key(key: &NewKey, cx: &App) -> impl IntoElement + use<> {
    let value = |id: &'static str, label: SharedString, value: &str| {
        v_flex()
            .gap_1()
            .child(div().font_weight(FontWeight::MEDIUM).child(label))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(code_line(value.to_string().into(), cx))
                    .child(
                        Clipboard::new(id)
                            .value(value.to_string())
                            .tooltip(t("copy")),
                    ),
            )
    };
    v_flex()
        .gap_3()
        .text_sm()
        .child(
            h_flex()
                .gap_2()
                .child(div().font_weight(FontWeight::MEDIUM).child(t("name")))
                .child(key.name.clone()),
        )
        .child(value("copy-address", t("address"), &key.address))
        .child(value("copy-key", t("key"), &key.key))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(t("key-shown-only-now")),
        )
}

/// One open session: the folder it works in, whether it speaks or listens,
/// and when it last did.
fn session_row(
    session: &AssistantSession,
    home: Option<&std::path::Path>,
    cx: &App,
) -> gpui_kit::Div {
    let theme = cx.theme();
    let name: SharedString = session
        .folder
        .as_deref()
        .map_or_else(|| t("session-unnamed"), |folder| folder_name(folder).into());
    let mut about = Vec::new();
    if let Some(folder) = &session.folder {
        about.push(shown_folder(folder, home));
    }
    if session.agents > 0 {
        about.push(t_with("session-agents", &[("count", &session.agents.to_string())]).to_string());
    }
    let state = if session.speaking {
        Some(StatusPill::new(Dot::Green, t("session-speaking")))
    } else if session.listening {
        Some(StatusPill::new(Dot::Yellow, t("session-listening")))
    } else {
        None
    };
    let ago = page::used_ago_now(session.last_used_at.as_deref()).map(|ago| match ago {
        Ago::Now => t("used-now"),
        Ago::Minutes(minutes) => t_with("used-minutes-ago", &[("minutes", &minutes.to_string())]),
        Ago::Hours(hours) => t_with("used-hours-ago", &[("hours", &hours.to_string())]),
    });
    h_flex()
        .w_full()
        .gap_3()
        .items_center()
        .px_3()
        .py_2()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().child(name))
                .when(!about.is_empty(), |text| {
                    text.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(about.join(" · ")),
                    )
                }),
        )
        .children(state)
        .when_some(ago, |row, ago| {
            row.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(ago),
            )
        })
}

/// The tile an assistant's name sits beside.
fn tile(cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    div()
        .mt_3()
        .flex_shrink_0()
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.radius)
        .bg(theme.muted)
        .text_color(theme.muted_foreground)
        .child(gpui_kit::component::Icon::new(IconName::Bot).small())
}

/// What the assistant can do through Super Voice.
fn does(id: AssistantId) -> SharedString {
    if page::reads_every_reply(id) {
        t("assistant-reads-replies")
    } else {
        t("assistant-reads-dictated-replies")
    }
}

/// A notice, in the app's words.
fn notice_words(notice: Notice) -> SharedString {
    match notice {
        Notice::CouldNotLoad(e) => t_with("assistants-load-failed", &[("error", &e)]),
        Notice::CouldNotConnect(id, e) => t_with(
            "assistant-connect-failed",
            &[("name", id.display_name()), ("error", &e)],
        ),
        Notice::CouldNotDisconnect(id, e) => t_with(
            "assistant-disconnect-failed",
            &[("name", id.display_name()), ("error", &e)],
        ),
        Notice::CouldNotChange(id, e) => t_with(
            "assistant-change-failed",
            &[("name", id.display_name()), ("error", &e)],
        ),
        Notice::CouldNotCreateKey(e) => t_with("key-create-failed", &[("error", &e)]),
        Notice::CouldNotRemoveKey(e) => t_with("key-remove-failed", &[("error", &e)]),
    }
}

/// The dialogs' width: 460 px, or the window less 32.
fn dialog_width(window: &Window) -> gpui_kit::Pixels {
    px(DIALOG_WIDTH).min(width::window_width(window) - px(32.))
}

/// A name for an assistant's elements that stays the same in every language.
const fn assistant_key(id: AssistantId) -> &'static str {
    match id {
        AssistantId::ClaudeCode => "claude-code",
        AssistantId::Codex => "codex",
        AssistantId::OpenCode => "opencode",
        AssistantId::Antigravity => "antigravity",
    }
}

/// A name for a switch's element.
const fn flag_key(flag: Flag) -> &'static str {
    match flag {
        Flag::ShowTalk => "show-talk",
        Flag::AutosendDictatedPrompts => "autosend",
        Flag::AnnounceWhenNeeded => "announce",
        Flag::ConversationContext => "context",
    }
}

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "address",
            "announce-when-needed",
            "announce-when-needed-description",
            "another-assistant",
            "any-mcp-assistant",
            "any-mcp-assistant-description",
            "assistant-can-listen",
            "assistant-can-speak",
            "assistant-change-failed",
            "assistant-connect-failed",
            "assistant-connected",
            "assistant-disconnect-failed",
            "assistant-not-found",
            "assistant-reads-dictated-replies",
            "assistant-reads-replies",
            "assistant-restart-to-use",
            "assistant-will-be-able-to",
            "assistants-intro",
            "assistants-intro-caption",
            "assistants-load-failed",
            "autosend-dictated-prompts",
            "autosend-dictated-prompts-description",
            "cancel",
            "connect",
            "connect-assistant-question",
            "connect-codex-caption",
            "connect-ellipsis",
            "connect-plugin-caption",
            "connecting-assistant",
            "copy",
            "create",
            "create-key",
            "create-key-ellipsis",
            "creating-key",
            "disconnect",
            "disconnect-assistant-description",
            "disconnect-assistant-question",
            "disconnect-ellipsis",
            "done",
            "key",
            "key-create-failed",
            "key-for-another-assistant",
            "key-name-caption",
            "key-remove-failed",
            "key-shown-only-now",
            "name",
            "on-this-computer",
            "remove",
            "remove-ellipsis",
            "remove-key-description",
            "remove-key-question",
            "session-agents",
            "session-listening",
            "session-speaking",
            "session-unnamed",
            "sessions-open",
            "show-talk-above-prompt",
            "show-talk-above-prompt-description",
            "today-at",
            "use-conversation-as-context",
            "use-conversation-as-context-description",
            "used-hours-ago",
            "used-minutes-ago",
            "used-now",
            "yesterday-at",
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
