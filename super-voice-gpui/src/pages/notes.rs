// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text › Notes: the notes the app keeps, spoken into it.
//!
//! The list (newest first, with a search) and the note open beside it, while
//! the content is 640 px wide; below, one at a time, the note with a back
//! button. Record makes a new note; Dictate more adds to the open one. A take
//! records until Stop: its words show as they are heard, muted, with the
//! daemon's microphone as a waveform and how long it has run. The note is a
//! plain editor, saved as it is typed; Export writes it where the user picks,
//! and Delete asks first, naming the note.
//!
//! What a press does is the core's (`pages::notes`): this draws its state,
//! keeps the editor's own buffer in step with it, and runs its work.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{
    Input as TextInput, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Selectable as _, Sizable as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::notes::Note;
use super_voice_app_core::pages::notes::{self as page, Effect, Input, Notice};
use super_voice_app_core::pages::readiness::{Feature, Readiness as FeatureReadiness};
use super_voice_app_core::state::Microphone;
use super_voice_app_core::state::notes::{NoteTake, NotesState, TakeTarget};

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand, SidebarMark, take};
use crate::shared::Shared;
use crate::ui::empty_state::EmptyState;
use crate::ui::list_detail::ListDetail;
use crate::ui::toast::{self, Toast};
use crate::ui::waveform::{Levels, Waveform};
use crate::ui::width;
use crate::{runtime, t, t_with};

/// The list's width beside the note: 300 px, 248 in a compact window.
const LIST_WIDE: Pixels = px(300.);
const LIST_COMPACT: Pixels = px(248.);
/// Content narrower than this has the narrower list.
const LIST_NARROWS: Pixels = px(900.);
/// A note's pane narrower than this keeps Export in its "…" menu.
const EXPORT_IN_MENU: Pixels = px(520.);
/// The editor grows with its note, from a few rows; the pane scrolls it, so
/// words being dictated show right after it.
const EDITOR_ROWS: usize = 1;
const EDITOR_MAX_ROWS: usize = 100_000;
/// How often a take's clock moves.
const TICK: Duration = Duration::from_millis(500);

/// The Notes page.
pub struct NotesPage {
    state: NotesState<String>,
    shared: Shared,
    microphone: Microphone,
    levels: Levels,
    search: Entity<InputState>,
    editor: Entity<TextareaState>,
    /// The note the editor holds, so opening another replaces its text.
    editor_holds: Option<String>,
    /// The take recording, while one is.
    take: Option<take::Take>,
    /// Moves the take's clock.
    ticker: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PageView for NotesPage {
    fn new(_: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t("search-notes")));
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .soft_wrap(true)
                .auto_grow(EDITOR_ROWS, EDITOR_MAX_ROWS)
        });
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&editor, window, Self::on_editor),
            cx.observe(&shared.readiness, |_, _, cx| cx.notify()),
        ];
        // The window is going: what the editor holds and every write still
        // waiting are written now, before the app ends.
        cx.on_release(|this: &mut Self, _| {
            let outcome = page::update(&mut this.state, Input::SaveBeforeExit);
            for effect in outcome.effects {
                if let Effect::WriteNow { jobs, new_note } = effect
                    && (!jobs.is_empty() || new_note.is_some())
                {
                    page::write_now(jobs, new_note);
                }
            }
        })
        .detach();
        Self {
            state: NotesState::default(),
            shared: shared.clone(),
            microphone: Microphone::default(),
            levels: Levels::default(),
            search,
            editor,
            editor_holds: None,
            take: None,
            ticker: None,
            _subscriptions: subscriptions,
        }
    }

    // The notes are the app's own files, but the page is read only once the
    // daemon answers: a screenshot, which never reaches it, must not show the
    // notes of whoever takes it.
    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(Input::Reload, window, cx);
    }

    fn heard(&mut self, event: &Event, _: &mut Window, cx: &mut Context<Self>) {
        if !self.microphone.follow(event) {
            return;
        }
        match *event {
            Event::Recording(true) => self.levels.clear(),
            Event::MicrophoneLevel { level, .. } if self.microphone.recording => {
                self.levels.push(level);
            }
            _ => {}
        }
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if width::content(window) >= width::TWO_PANES || self.state.open.is_none() {
            return false;
        }
        self.send(Input::Close, window, cx);
        true
    }

    // A take recording holds the window: it stops, its words are written,
    // then the window closes (the core's `FinishBeforeExit`).
    fn closing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.state.take.is_none() {
            return false;
        }
        self.send(Input::FinishBeforeExit, window, cx);
        true
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let notes = t("notes");
        std::iter::once(PaletteCommand::new(
            "new-note",
            t("new-note"),
            IconName::Mic,
        ))
        .chain(self.state.items.iter().map(|note| {
            PaletteCommand::new(
                format!("open:{}", note.id),
                page::first_line(&note.text, 60),
                Lucide::NotebookPen,
            )
            .group(notes.clone())
        }))
        .collect()
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == "new-note" {
            if self.can_record(cx) {
                self.send(Input::Record, window, cx);
            }
        } else if let Some(note) = id.strip_prefix("open:") {
            self.send(Input::Open(note.to_string()), window, cx);
        }
    }

    fn sidebar_mark(&self, _: &App) -> Option<SidebarMark> {
        Some(SidebarMark::Count(self.state.items.len()))
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        pretend::fill(self, scenes, window, cx);
    }
}

impl CorePage for NotesPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let exported = match &input {
            Input::Exported(Ok(Some(path))) => Some(path.clone()),
            _ => None,
        };
        let outcome = page::update(&mut self.state, input);
        self.sync_editor(window, cx);
        effects::run_effects(self, outcome.effects, window, cx);
        let notice = outcome.notice.map(|notice| toast_for(&notice));
        let notice = notice.or_else(|| {
            exported.map(|path| {
                Toast::success(t_with("exported-to", &[("path", &path)])).title(t("note-exported"))
            })
        });
        toast::notice::<Self>(window, cx, outcome.clear_notice, notice);
        if self.state.take.is_none() {
            self.take = None;
            self.ticker = None;
        } else if self.ticker.is_none() {
            self.ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
            }));
        }
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        match effect {
            Effect::List => Some(runner(page::run_list())),
            Effect::Write(job) => Some(runner(page::run_write(job))),
            Effect::Create(text) => Some(runner(page::run_create(text))),
            Effect::SaveAfter(edits) => Some(runner(page::run_save_after(edits))),
            Effect::GiveUpLater => Some(runner(page::run_give_up())),
            Effect::StartTake => {
                self.start_take(window, cx);
                None
            }
            Effect::StopTake => {
                drop(runtime::spawn(page::run_stop()));
                None
            }
            Effect::Export { name, text } => {
                Self::export(&name, text, window, cx);
                None
            }
            Effect::WriteNow { jobs, new_note } => {
                page::write_now(jobs, new_note);
                None
            }
            Effect::Exit => {
                self.shared.nav.finished_closing(Page::Notes, window, cx);
                None
            }
        }
    }
}

/// A notice as a toast: a failure as an error; a take that heard nothing
/// as information, since nothing failed; a take refused because another
/// recording holds the microphone as a warning.
fn toast_for(notice: &Notice) -> Toast {
    let text = notice_text(notice);
    match notice {
        Notice::NoSpeech => Toast::info(text),
        Notice::AnotherRecording => Toast::warning(text),
        Notice::CouldNotRead(_)
        | Notice::CouldNotSave(_)
        | Notice::CouldNotDelete(_)
        | Notice::CouldNotExport(_)
        | Notice::RecordingFailed(_) => Toast::error(text),
    }
}

/// A notice, in the app's words.
fn notice_text(notice: &Notice) -> SharedString {
    let failed = |key: &str, error: &str| t_with(key, &[("error", error)]);
    match notice {
        Notice::CouldNotRead(e) => failed("notes-could-not-be-read", e),
        Notice::CouldNotSave(e) => failed("the-note-could-not-be-saved", e),
        Notice::CouldNotDelete(e) => failed("the-note-could-not-be-deleted", e),
        Notice::CouldNotExport(e) => failed("the-note-could-not-be-exported", e),
        Notice::NoSpeech => t("no-speech-was-heard"),
        Notice::AnotherRecording => t("another-recording-is-running"),
        Notice::RecordingFailed(e) => failed("the-recording-failed", e),
    }
}

impl NotesPage {
    /// Record a take: each thing it hears comes back as an input, until its
    /// transcript.
    fn start_take(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.levels.clear();
        self.take = Some(take::read(
            page::take(),
            window,
            cx,
            |page, input, window, cx| {
                page.send(input, window, cx);
            },
        ));
    }

    /// Ask where to export the note, then write it there.
    fn export(name: &str, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_new_path(&page::export_start(), Some(name));
        cx.spawn_in(window, async move |this, cx| {
            let input = match chosen.await {
                Ok(Ok(Some(path))) => runtime::spawn(page::run_export_to(path, text))
                    .await
                    .unwrap_or_else(|e| Input::Exported(Err(e.to_string()))),
                Ok(Ok(None)) | Err(_) => Input::Exported(Ok(None)),
                Ok(Err(e)) => Input::Exported(Err(e.to_string())),
            };
            _ = this.update_in(cx, |page, window, cx| page.send(input, window, cx));
        })
        .detach();
    }

    /// The editor changed: its text is the open note's.
    fn on_editor(
        &mut self,
        editor: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let text = editor.read(cx).value().to_string();
        let Some(open) = self.state.open.as_mut() else {
            return;
        };
        if open.content != text {
            open.content = text;
            self.send(Input::Edited, window, cx);
        }
    }

    /// Keep the editor's buffer and the open note in step: another note
    /// opened, or words dictated onto the end of this one.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.state.open.as_ref() else {
            self.editor_holds = None;
            return;
        };
        let same = self.editor_holds.as_deref() == Some(open.id.as_str());
        if same && self.editor.read(cx).value().as_ref() == open.content {
            return;
        }
        let text = open.content.clone();
        self.editor_holds = Some(open.id.clone());
        self.editor.update(cx, |editor, cx| {
            editor.set_value(text.clone(), window, cx);
            // Dictated words arrive at the end, where the cursor goes to
            // carry on.
            if same {
                let line = text.lines().count().saturating_sub(1);
                let column = text.lines().last().map_or(0, |l| l.encode_utf16().count());
                editor.set_cursor_position(
                    gpui_kit::component::input::Position::new(
                        u32::try_from(line).unwrap_or(u32::MAX),
                        u32::try_from(column).unwrap_or(u32::MAX),
                    ),
                    window,
                    cx,
                );
            }
        });
    }

    /// Why a take can't start now, or `None` when it can.
    fn record_blocked(&self, cx: &App) -> Option<SharedString> {
        if self.state.take.is_some() || self.microphone.recording {
            return Some(t("another-recording-is-running"));
        }
        let readiness = self.shared.readiness.read(cx).of(Feature::SpeechToText);
        (!matches!(readiness, FeatureReadiness::Ready | FeatureReadiness::Idle))
            .then(|| t("speech-to-text-is-not-ready"))
    }

    fn can_record(&self, cx: &App) -> bool {
        self.record_blocked(cx).is_none()
    }

    /// A press that sends `input`.
    fn press(
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _, window, cx| this.send(input.clone(), window, cx))
    }

    /// Ask before deleting the open note, naming it.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .state
            .open
            .as_ref()
            .and_then(|open| self.state.note(&open.id))
            .map(page::short_name)
        else {
            return;
        };
        self.send(Input::DeleteRequested, window, cx);
        let this = cx.entity().downgrade();
        let title = t_with("delete-note-title", &[("name", &name)]);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let ok = this.clone();
            let cancel = this.clone();
            alert
                .title(title.clone())
                .description(t("delete-note-body"))
                .confirm()
                .ok_text(t("delete"))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t("cancel"))
                .on_ok(move |_, window, cx| {
                    _ = ok.update(cx, |page, cx| page.send(Input::DeleteConfirmed, window, cx));
                    true
                })
                .on_cancel(move |_, window, cx| {
                    _ = cancel.update(cx, |page, cx| page.send(Input::DeleteCancelled, window, cx));
                    true
                })
        });
    }
}

impl Render for NotesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = width::content(window);
        let two_panes = content >= width::TWO_PANES;
        let list_width = if content >= LIST_NARROWS {
            LIST_WIDE
        } else {
            LIST_COMPACT
        };
        let pane = if two_panes {
            content - list_width
        } else {
            content
        };
        let list = self.list(two_panes, cx);
        let (detail, back_actions) = self.detail(pane, two_panes, cx);
        let placeholder = self.placeholder(cx);
        let mut panes = ListDetail::new("notes", list)
            .list_width(list_width)
            .detail(detail)
            .placeholder(placeholder);
        // A take recording a new note has no list to go back to until it
        // stops.
        if self.state.open.is_some()
            && self
                .state
                .take
                .as_ref()
                .is_none_or(|take| take.target != TakeTarget::New)
        {
            panes = panes.back(t("notes"), Self::press(Input::Close, cx));
        }
        if let Some(actions) = back_actions {
            panes = panes.back_actions(actions);
        }
        panes
    }
}

// The list.
impl NotesPage {
    fn record_button(&self, cx: &mut Context<Self>) -> Button {
        let blocked = self.record_blocked(cx);
        Button::new("record")
            .primary()
            .icon(IconName::Mic)
            .label(t("record"))
            .disabled(blocked.is_some())
            .when_some(blocked, Button::tooltip)
            .on_click(Self::press(Input::Record, cx))
    }

    fn list(&self, two_panes: bool, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let search = TextInput::new(&self.search)
            .prefix(gpui_kit::component::Icon::new(IconName::Search).small())
            .cleanable(true);
        let header = if two_panes {
            v_flex()
                .gap_2()
                .px_3()
                .pt_3()
                .pb_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t("notes")),
                        )
                        .child(self.record_button(cx)),
                )
                .child(search)
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .px_4()
                .pt_3()
                .pb_2()
                .child(div().flex_1().min_w_0().child(search))
                .child(self.record_button(cx))
                .into_any_element()
        };
        let query = self.search.read(cx).value();
        let today = page::today();
        let open = self.state.open.as_ref().map(|o| o.id.as_str());
        let notes = page::matching(&self.state.items, &query);
        let new_take = self
            .state
            .take
            .as_ref()
            .filter(|take| take.target == TakeTarget::New)
            .map(|take| Self::take_row(take, cx));
        let body: AnyElement = if !self.state.loaded {
            div()
                .px_4()
                .py_2()
                .text_sm()
                .text_color(muted)
                .child(t("loading-progress"))
                .into_any_element()
        } else if self.state.items.is_empty() && new_take.is_none() {
            // Beside the list, the empty pane says it; alone, the list does.
            if two_panes {
                div().into_any_element()
            } else {
                self.empty(cx).into_any_element()
            }
        } else if notes.is_empty() && new_take.is_none() {
            div()
                .px_4()
                .py_2()
                .text_sm()
                .text_color(muted)
                .child(t("no-notes-match"))
                .into_any_element()
        } else {
            v_flex()
                .gap_0p5()
                .children(new_take)
                .children(notes.into_iter().map(|note| {
                    let about = page::row_about(note, today);
                    Self::note_row(note, about, open == Some(note.id.as_str()), cx)
                }))
                .into_any_element()
        };
        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("notes-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .child(body),
            )
            .into_any_element()
    }

    /// A note in the list: how it starts, and when it was made and how long
    /// it is.
    fn note_row(note: &Note, about: String, selected: bool, cx: &mut Context<Self>) -> ListItem {
        let muted = cx.theme().muted_foreground;
        ListItem::new(SharedString::from(format!("note-{}", note.id)))
            .selected(selected)
            .accessibility_label(page::first_line(&note.text, page::ROW_PREVIEW_CHARS))
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .py_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(page::first_line(&note.text, page::ROW_PREVIEW_CHARS)),
                    )
                    .child(div().text_xs().text_color(muted).truncate().child(about)),
            )
            .on_click(Self::press(Input::Open(note.id.clone()), cx))
    }

    /// The new note a take is recording, at the top of the list.
    fn take_row(take: &NoteTake, cx: &mut Context<Self>) -> ListItem {
        let theme = cx.theme();
        let elapsed = t_with(
            "recording-elapsed",
            &[
                ("elapsed", &page::clock(take.started.elapsed())),
                ("limit", &page::clock(page::take_limit())),
            ],
        );
        ListItem::new("note-recording").selected(true).child(
            h_flex()
                .w_full()
                .gap_2()
                .items_start()
                .child(
                    gpui_kit::component::Icon::new(IconName::Mic)
                        .small()
                        .mt_0p5()
                        .text_color(theme.primary),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .py_0p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(t("new-note")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(elapsed),
                        ),
                ),
        )
    }

    /// No notes yet, and what to do.
    fn empty(&self, cx: &mut Context<Self>) -> EmptyState {
        EmptyState::new(Lucide::NotebookPen, t("no-notes-yet"))
            .description(t("each-note-you-record-shows-here"))
            .action(self.record_button(cx).small())
    }

    /// What the note's pane shows beside the list while no note is open.
    fn placeholder(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = if self.state.loaded && self.state.items.is_empty() {
            self.empty(cx)
        } else {
            EmptyState::new(Lucide::NotebookPen, t("no-note-open")).description(t("open-a-note"))
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(empty)
            .into_any_element()
    }
}

// The note, or the take recording a new one.
impl NotesPage {
    /// The detail beside the list, or alone with the back button; and, when
    /// alone, the actions that go on the back button's row.
    fn detail(
        &self,
        pane: Pixels,
        two_panes: bool,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let new_take = self
            .state
            .take
            .as_ref()
            .filter(|take| take.target == TakeTarget::New);
        if let Some(take) = new_take {
            return (Some(self.new_take(take, two_panes, cx)), None);
        }
        let Some(open) = self.state.open.as_ref() else {
            return (None, None);
        };
        let title: SharedString = self.state.note(&open.id).map_or_else(
            || open.id.clone().into(),
            |note| page::when(note.created, page::today()).into(),
        );
        let about = page::open_about(&self.state, open);
        let take = self.state.take.as_ref();
        let adding_here =
            take.is_some_and(|take| take.target == TakeTarget::Append(open.id.clone()));
        let actions = self.note_actions(adding_here, pane < EXPORT_IN_MENU || !two_panes, cx);
        let name = Self::name_block(title, about, cx);
        let theme = cx.theme();
        let top = if two_panes {
            h_flex()
                .gap_2()
                .min_h(px(56.))
                .pl_4()
                .pr_3()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(name)
                .child(actions)
                .into_any_element()
        } else {
            div().px_4().pt_3().child(name).into_any_element()
        };
        let live = take
            .filter(|_| adding_here)
            .map(|take| take.preview.clone())
            .filter(|words| !words.is_empty());
        let editor = self.editor.clone();
        let body = v_flex()
            .id("note-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .pt_2()
            .pb_6()
            .cursor_text()
            // A click below the words puts the cursor in the note.
            .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                editor.update(cx, |editor, cx| editor.focus(window, cx));
            })
            .child(
                Textarea::new(&self.editor)
                    .appearance(false)
                    .aria_label(t("notes")),
            )
            .when_some(live, |body, words| {
                body.child(
                    div()
                        .px_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(words),
                )
            });
        // A take into another note than this one says which.
        let label = take.and_then(|take| match &take.target {
            TakeTarget::Append(id) if !adding_here => self.state.note(id).map(|note| {
                t_with(
                    "adding-to-note",
                    &[("when", &page::when(note.created, page::today()))],
                )
            }),
            _ => None,
        });
        let detail = v_flex()
            .size_full()
            .child(top)
            .child(body)
            .children(take.map(|take| self.record_bar(take, label, cx)))
            .into_any_element();
        let back_actions =
            (!two_panes).then(|| self.note_actions(adding_here, true, cx).into_any_element());
        (Some(detail), back_actions)
    }

    /// The open note's name, and the line under it.
    fn name_block(title: SharedString, about: String, cx: &App) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(about),
            )
    }

    /// Dictate more, Export and the "…" menu with Delete. Export goes into the
    /// menu when the pane is narrow.
    fn note_actions(
        &self,
        adding_here: bool,
        export_in_menu: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let blocked = self.record_blocked(cx).filter(|_| !adding_here);
        let dictate = Button::new("dictate-more")
            .icon(IconName::Mic)
            .label(t("dictate-more"))
            .selected(adding_here)
            .disabled(blocked.is_some())
            .when_some(blocked, Button::tooltip)
            .on_click(Self::press(Input::DictateMore, cx));
        let export = (!export_in_menu).then(|| {
            Button::new("export")
                .label(t("export"))
                .on_click(Self::press(Input::Export, cx))
        });
        let this = cx.entity().downgrade();
        let more = Button::new("note-more")
            .ghost()
            .icon(IconName::Ellipsis)
            .accessibility_label(t("more-actions"))
            .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, _, _| {
                let export = this.clone();
                let delete = this.clone();
                menu.when(export_in_menu, |menu| {
                    menu.item(
                        PopupMenuItem::new(t("export")).on_click(move |_, window, cx| {
                            _ = export.update(cx, |page, cx| page.send(Input::Export, window, cx));
                        }),
                    )
                })
                .item(
                    PopupMenuItem::new(t("delete-ellipsis")).on_click(move |_, window, cx| {
                        _ = delete.update(cx, |page, cx| page.confirm_delete(window, cx));
                    }),
                )
            });
        h_flex()
            .flex_shrink_0()
            .gap_2()
            .child(dictate)
            .children(export)
            .child(more)
    }

    /// A take recording a new note: its words as they come, and the bar.
    fn new_take(&self, take: &NoteTake, two_panes: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let about = t_with(
            "recording-elapsed",
            &[
                ("elapsed", &page::clock(take.started.elapsed())),
                ("limit", &page::clock(page::take_limit())),
            ],
        );
        let name = Self::name_block(t("new-note"), about.to_string(), cx);
        let top = if two_panes {
            h_flex()
                .min_h(px(56.))
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(name)
                .into_any_element()
        } else {
            div().px_4().pt_3().child(name).into_any_element()
        };
        let words: SharedString = if take.preview.is_empty() {
            t("listening")
        } else {
            take.preview.clone().into()
        };
        v_flex()
            .size_full()
            .child(top)
            .child(
                div()
                    .id("new-note-words")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_5()
                    .py_4()
                    .text_base()
                    .text_color(theme.muted_foreground)
                    .child(words),
            )
            .child(self.record_bar(take, None, cx))
            .into_any_element()
    }

    /// Under a take: what it adds to when that isn't the note shown, the
    /// microphone, how long it has run, and Stop.
    fn record_bar(
        &self,
        take: &NoteTake,
        label: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);
        let stop = Button::new("stop-take")
            .primary()
            .icon(IconName::Square)
            .label(if take.stopping {
                t("stopping")
            } else {
                t("stop")
            })
            .loading(take.stopping)
            .disabled(take.stopping)
            .on_click(Self::press(Input::Stop, cx));
        h_flex()
            .gap_3()
            .px_4()
            .py_2p5()
            .border_t_1()
            .border_color(border)
            .when_some(label, |bar, label| {
                bar.child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(160.))
                        .text_xs()
                        .text_color(muted)
                        .child(label),
                )
            })
            .child(Waveform::new(&self.levels).live(self.microphone.recording))
            .child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(muted)
                    .child(page::take_elapsed(take)),
            )
            .child(stop)
            .into_any_element()
    }
}

#[cfg(feature = "screenshot")]
mod pretend;

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "adding-to-note",
            "another-recording-is-running",
            "cancel",
            "delete",
            "delete-ellipsis",
            "delete-note-body",
            "delete-note-title",
            "dictate-more",
            "each-note-you-record-shows-here",
            "export",
            "exported-to",
            "listening",
            "loading-progress",
            "more-actions",
            "new-note",
            "no-note-open",
            "no-notes-match",
            "no-notes-yet",
            "no-speech-was-heard",
            "note-exported",
            "notes",
            "notes-could-not-be-read",
            "open-a-note",
            "record",
            "recording-elapsed",
            "search-notes",
            "speech-to-text-is-not-ready",
            "stop",
            "stopping",
            "take-elapsed",
            "the-note-could-not-be-deleted",
            "the-note-could-not-be-exported",
            "the-note-could-not-be-saved",
            "the-recording-failed",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
