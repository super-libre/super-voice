// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text › Contexts: what a model is told about what is dictated.
//!
//! The list of contexts, each with a radio that puts it in use (and No
//! context, which sends nothing), and the editor of the one picked beside it:
//! its name, its words to listen for (one field each, Enter for the next),
//! and its prompt. Below 640 px the editor is shown alone, with a back
//! button. Delete asks first, naming the context.
//!
//! What a press does is the core's (`pages::contexts`); the daemon owns the
//! list, and this keeps the editor's fields in step with the core's draft.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{
    Input as TextInput, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};
use super_voice_app_core::events::{Event, setting_changed};
use super_voice_app_core::pages::contexts::{self as page, Effect, Input, Notice, Reach};
use super_voice_app_core::state::contexts::{ContextsState, RowId};
use super_voice_shared::models::contexts::DictationContext;

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand};
use crate::shared::Shared;
use crate::ui::empty_state::EmptyState;
use crate::ui::key_list::{self, KeyList, KeyListState};
use crate::ui::list_detail::ListDetail;
use crate::ui::page_header::PageHeader;
use crate::ui::status::{Dot, StatusPill};
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, t_with};

/// The list's width beside the editor: 300 px, 248 in a compact window.
const LIST_WIDE: Pixels = px(300.);
const LIST_COMPACT: Pixels = px(248.);
const LIST_NARROWS: Pixels = px(900.);
/// The editor's widest.
const EDITOR_MAX: Pixels = px(640.);

/// The No context row's id in the keyboard's list: no context's id is
/// empty.
const NONE_ROW: &str = "";

/// The Contexts page.
pub struct ContextsPage {
    state: ContextsState<String>,
    /// How far a context reaches among the installed backends.
    reach: Option<Reach>,
    name: Entity<InputState>,
    prompt: Entity<TextareaState>,
    /// One field per vocabulary row of the draft, in its order.
    terms: Vec<Term>,
    /// The context the editor's fields hold.
    editing: Option<String>,
    /// The list, as the keyboard reaches it.
    keys: KeyListState,
    shared: Shared,
    _subscriptions: Vec<Subscription>,
}

/// A vocabulary row's field.
struct Term {
    row: RowId,
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl PageView for ContextsPage {
    fn new(_: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("context-name-placeholder")));
        let prompt = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let subscriptions = vec![
            cx.subscribe_in(
                &name,
                window,
                |this, name, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = name.read(cx).value().to_string();
                        this.send(Input::NameChanged(value), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &prompt,
                window,
                |this, prompt, event: &InputEvent, window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let value = prompt.read(cx).value().to_string();
                    if let Some(draft) = this.state.draft.as_mut()
                        && draft.prompt != value
                    {
                        draft.prompt = value;
                        this.send(Input::PromptEdited, window, cx);
                    }
                },
            ),
        ];
        Self {
            state: ContextsState::default(),
            reach: None,
            name,
            prompt,
            terms: Vec::new(),
            editing: None,
            keys: KeyListState::new(cx),
            shared: shared.clone(),
            _subscriptions: subscriptions,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(Input::Reload, window, cx);
        Self::read_reach(cx);
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        // Another client changed a context, or this one did and it comes
        // back: read the list again. The open editor stays as typed.
        if setting_changed(event) == Some("contexts") {
            self.send(Input::Reload, window, cx);
        }
        // A backend installed changes how far a context reaches. Only once
        // it is in: an install's progress comes many times a second.
        if page::changes_reach(event) {
            Self::read_reach(cx);
        }
    }

    /// A backend removed sends no event: read how far a context reaches
    /// each time the page is shown.
    fn shown(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if !self.shared.connection.read(cx).is_down() {
            Self::read_reach(cx);
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if width::content(window) >= width::TWO_PANES || self.state.draft.is_none() {
            return false;
        }
        self.send(Input::CancelEdit, window, cx);
        true
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let contexts = t("contexts");
        std::iter::once(PaletteCommand::new(
            "new-context",
            t("new-context"),
            IconName::Plus,
        ))
        .chain(
            self.state
                .items
                .iter()
                .filter(|c| self.state.active.as_deref() != Some(c.id.as_str()))
                .map(|context| {
                    PaletteCommand::new(
                        format!("use:{}", context.id),
                        t_with("use-context", &[("name", &context.display_name())]),
                        Lucide::BookOpenText,
                    )
                    .group(contexts.clone())
                }),
        )
        .collect()
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == "new-context" {
            self.send(Input::Create, window, cx);
        } else if let Some(context) = id.strip_prefix("use:") {
            self.send(Input::Activate(Some(context.to_string())), window, cx);
        }
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        pretend::fill(self, scenes, window, cx);
    }
}

impl KeyList for ContextsPage {
    fn key_list(&self) -> &KeyListState {
        &self.keys
    }

    fn key_list_mut(&mut self) -> &mut KeyListState {
        &mut self.keys
    }

    /// The contexts, then No context.
    fn rows(&self, _: &App) -> Vec<String> {
        self.state
            .items
            .iter()
            .map(|context| context.id.clone())
            .chain(std::iter::once(NONE_ROW.to_string()))
            .collect()
    }

    fn picked(&self) -> Option<String> {
        self.state.draft.as_ref().map(|draft| draft.id.clone())
    }

    /// Enter opens a context's editor, as a click does; on No context, it
    /// puts no context in use, as a click does.
    fn open_row(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = if id == NONE_ROW {
            Input::Activate(None)
        } else {
            Input::Edit(id.to_string())
        };
        self.send(input, window, cx);
    }
}

impl CorePage for ContextsPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        self.sync_editor(window, cx);
        effects::run_effects(self, outcome.effects, window, cx);
        toast::notice::<Self>(
            window,
            cx,
            outcome.clear_notice,
            outcome.notice.map(|notice| {
                Toast::error(match notice {
                    Notice::CouldNotLoad(e) => t_with("contexts-could-not-load", &[("error", &e)]),
                    Notice::CouldNotSave(e) => t_with("context-could-not-save", &[("error", &e)]),
                })
            }),
        );
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
            Effect::Activate(id) => Some(runner(page::run_activate(id))),
            Effect::Delete(id) => Some(runner(page::run_delete(id))),
            Effect::Save(context) => Some(runner(page::run_save(context))),
            Effect::FocusTerm(row) => {
                if let Some(term) = self.terms.iter().find(|term| term.row == row) {
                    term.input.update(cx, |input, cx| input.focus(window, cx));
                }
                None
            }
            Effect::OpenEditor => {
                if self.state.draft.as_ref().is_some_and(|d| d.is_new) {
                    self.name.update(cx, |name, cx| name.focus(window, cx));
                }
                None
            }
            Effect::CloseEditor => None,
        }
    }
}

impl ContextsPage {
    /// Read how far a context reaches among the installed backends.
    fn read_reach(cx: &mut Context<Self>) {
        effects::spawn(page::run_read_reach(), cx, |this, read, cx| {
            match read {
                Ok(reach) => this.reach = reach,
                Err(e) => log::warn!("Could not read the backends for the contexts: {e}"),
            }
            cx.notify();
        })
        .detach();
    }

    /// Keep the editor's fields in step with the core's draft: another
    /// context opened, or rows the core added, split or removed.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.state.draft.as_ref() else {
            self.editing = None;
            self.terms.clear();
            return;
        };
        if self.editing.as_deref() != Some(draft.id.as_str()) {
            self.editing = Some(draft.id.clone());
            self.terms.clear();
            let (name, prompt) = (draft.name.clone(), draft.prompt.clone());
            self.name
                .update(cx, |input, cx| input.set_value(name, window, cx));
            self.prompt
                .update(cx, |input, cx| input.set_value(prompt, window, cx));
        }
        let rows: Vec<(RowId, String)> = draft.terms.clone();
        let mut kept = std::mem::take(&mut self.terms);
        for (row, value) in rows {
            let term = match kept.iter().position(|term| term.row == row) {
                Some(at) => {
                    let term = kept.swap_remove(at);
                    if term.input.read(cx).value().as_ref() != value {
                        term.input
                            .update(cx, |input, cx| input.set_value(value, window, cx));
                    }
                    term
                }
                None => Self::term(row, &value, window, cx),
            };
            self.terms.push(term);
        }
    }

    /// A field for the vocabulary row `row`.
    fn term(row: RowId, value: &str, window: &mut Window, cx: &mut Context<Self>) -> Term {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("contexts-add-a-word"))
                .default_value(value.to_string())
        });
        let subscription =
            cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                let Some(index) = this.terms.iter().position(|term| term.row == row) else {
                    return;
                };
                match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        this.send(Input::TermChanged { index, value }, window, cx);
                    }
                    InputEvent::PressEnter { .. } => {
                        this.send(Input::TermSubmitted(index), window, cx);
                    }
                    _ => {}
                }
            });
        Term {
            row,
            input,
            _subscription: subscription,
        }
    }

    /// A press that sends `input`.
    fn press(
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _, window, cx| this.send(input.clone(), window, cx))
    }

    /// Ask before deleting the context being edited, naming it.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(context) = self
            .state
            .draft
            .as_ref()
            .and_then(|draft| self.state.get(&draft.id))
        else {
            return;
        };
        let (id, name) = (context.id.clone(), context.display_name());
        self.send(Input::DeleteRequested(id.clone()), window, cx);
        let this = cx.entity().downgrade();
        let title = t_with("delete-context-title", &[("name", &name)]);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (ok, cancel, id) = (this.clone(), this.clone(), id.clone());
            alert
                .title(title.clone())
                .description(t("delete-context-body"))
                .confirm()
                .ok_text(t("delete"))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t("cancel"))
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    _ = ok.update(cx, |page, cx| {
                        page.send(Input::DeleteConfirmed(id), window, cx);
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    _ = cancel.update(cx, |page, cx| page.send(Input::DeleteCancelled, window, cx));
                    true
                })
        });
    }
}

impl Render for ContextsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = width::content(window);
        let two_panes = content >= width::TWO_PANES;
        let list_width = if content >= LIST_NARROWS {
            LIST_WIDE
        } else {
            LIST_COMPACT
        };
        let new = Button::new("new-context")
            .icon(IconName::Plus)
            .label(t("new-context"))
            .on_click(Self::press(Input::Create, cx));
        let list = self.list(window, cx);
        let editor = self.editor(two_panes, window, cx);
        let placeholder = self.placeholder(cx);
        let mut panes = ListDetail::new("contexts", list)
            .list_width(list_width)
            .detail(editor)
            .placeholder(placeholder);
        if let Some(draft) = self.state.draft.as_ref() {
            panes = panes.back(t("contexts"), Self::press(Input::CancelEdit, cx));
            // Alone, the editor's Save sits on the back button's row too, in
            // reach without scrolling.
            panes = panes.back_actions(
                Button::new("save-context-top")
                    .primary()
                    .small()
                    .label(t("save"))
                    .loading(draft.saving)
                    .disabled(draft.saving)
                    .on_click(Self::press(Input::Save, cx)),
            );
        }
        // Alone, the editor takes the whole page: the header's New context
        // waits until it is closed.
        let header = (two_panes || self.state.draft.is_none()).then(|| {
            PageHeader::new(Page::Contexts)
                .description(t("a-context-tells-a-model-what-you"))
                .action(new)
        });
        v_flex().size_full().children(header).child(
            div()
                .flex_1()
                .min_h_0()
                .when(two_panes, |panes| {
                    panes.border_t_1().border_color(cx.theme().border)
                })
                .child(panes),
        )
    }
}

// The list.
impl ContextsPage {
    fn list(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let editing = self.state.draft.as_ref().map(|d| d.id.clone());
        let active = self.state.active.clone();
        let ringed = key_list::ringed(self, window, cx);
        let ring = |id: &str| ringed.as_deref() == Some(id);
        let reach = self.reach.map(|reach| {
            div()
                .px_2()
                .pt_3()
                .text_xs()
                .text_color(muted)
                .child(page::reach_text(reach))
        });
        // The rows are the list's own children, for the keyboard to scroll
        // to.
        let list = key_list::wire(div().id("contexts-list"), self, cx)
            .size_full()
            .overflow_y_scroll()
            .p_2();
        if !self.state.loaded {
            return list
                .child(
                    div()
                        .px_2()
                        .py_2()
                        .text_sm()
                        .text_color(muted)
                        .child(t("loading-progress")),
                )
                .into_any_element();
        }
        let mut rows: Vec<AnyElement> = self
            .state
            .items
            .iter()
            .map(|context| {
                let in_use = active.as_deref() == Some(context.id.as_str());
                let picked = editing.as_deref() == Some(context.id.as_str());
                let row = Self::context_row(context, in_use, picked, cx);
                key_list::row(row, ring(&context.id), window, cx).into_any_element()
            })
            .collect();
        let none = Self::none_row(active.is_none(), cx);
        rows.push(key_list::row(none, ring(NONE_ROW), window, cx).into_any_element());
        list.flex()
            .flex_col()
            .gap_0p5()
            .children(rows)
            .children(reach)
            .into_any_element()
    }

    /// A context in the list: the radio that puts it in use, its name and
    /// first words, and whether it is in use. The row opens its editor.
    fn context_row(
        context: &DictationContext,
        in_use: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> ListItem {
        let muted = cx.theme().muted_foreground;
        let name = context.display_name();
        let activate = cx.listener({
            let id = context.id.clone();
            move |this, _: &bool, window, cx| {
                cx.stop_propagation();
                this.send(Input::Activate(Some(id.clone())), window, cx);
            }
        });
        ListItem::new(SharedString::from(format!("context-{}", context.id)))
            .selected(picked)
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .py_0p5()
                    .child(
                        Radio::new(SharedString::from(format!("use-{}", context.id)))
                            .checked(in_use)
                            .accessibility_label(t_with("use-context", &[("name", &name)]))
                            .on_click(activate),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(page::row_summary(context)),
                            ),
                    )
                    .when(in_use, |row| {
                        row.child(StatusPill::new(Dot::Green, t("in-use")))
                    }),
            )
            .on_click(Self::press(Input::Edit(context.id.clone()), cx))
    }

    /// No context: nothing is sent. Not a context, so nothing to edit.
    fn none_row(in_use: bool, cx: &mut Context<Self>) -> ListItem {
        let muted = cx.theme().muted_foreground;
        ListItem::new("context-none")
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .py_0p5()
                    .child(
                        Radio::new("use-none")
                            .checked(in_use)
                            .accessibility_label(t("use-none"))
                            .on_click(cx.listener(|this, _: &bool, window, cx| {
                                this.send(Input::Activate(None), window, cx);
                            })),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().child(t("no-context")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t("no-context-sends-nothing")),
                            ),
                    )
                    .when(in_use, |row| {
                        row.child(StatusPill::new(Dot::Green, t("in-use")))
                    }),
            )
            .on_click(Self::press(Input::Activate(None), cx))
    }

    /// Beside the list while nothing is being edited.
    fn placeholder(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = if self.state.loaded && self.state.items.is_empty() {
            EmptyState::new(Lucide::BookOpenText, t("no-contexts-yet"))
                .description(t("contexts-add-one"))
                .action(
                    Button::new("new-context-empty")
                        .primary()
                        .small()
                        .icon(IconName::Plus)
                        .label(t("new-context"))
                        .on_click(Self::press(Input::Create, cx)),
                )
        } else {
            EmptyState::new(Lucide::BookOpenText, t("context-pick-one"))
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

// The editor.
impl ContextsPage {
    fn editor(
        &self,
        two_panes: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self.state.draft.as_ref()?;
        let theme = cx.theme();
        let (muted, danger, border) = (theme.muted_foreground, theme.danger, theme.border);
        let padding = if two_panes {
            px(24.)
        } else {
            width::page_padding(window)
        };
        let section = |title: SharedString, description: Option<SharedString>| {
            v_flex()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .when_some(description, |block, description| {
                    block.child(div().text_xs().text_color(muted).child(description))
                })
        };
        let terms = self.term_rows(cx);
        let actions = Self::editor_actions(draft.is_new, draft.saving, cx);
        let form = v_flex()
            .w_full()
            .max_w(EDITOR_MAX)
            .gap_5()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if draft.is_new {
                        t("new-context")
                    } else {
                        t_with("context-edit", &[("name", &draft.name)])
                    }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section(t("name"), None))
                    .child(TextInput::new(&self.name)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section(
                        t("words-to-listen-for"),
                        Some(t("names-jargon-anything-the-model-keeps-mishearing")),
                    ))
                    .children(terms),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section(t("prompt"), Some(t("context-prompt-description"))))
                    .child(Textarea::new(&self.prompt)),
            )
            .when_some(draft.error.clone(), |form, error| {
                form.child(div().text_sm().text_color(danger).child(error))
            })
            .child(div().border_t_1().border_color(border).child(actions));
        Some(
            div()
                .id("context-editor")
                .size_full()
                .overflow_y_scroll()
                .px(padding)
                .py_5()
                .child(form)
                .into_any_element(),
        )
    }

    /// One field per word, with a button that removes it; the last, empty
    /// one is where the next word goes.
    fn term_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let last = self.terms.len().saturating_sub(1);
        let terms: Vec<AnyElement> = self
            .terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                let remove = (index != last).then(|| {
                    Button::new(SharedString::from(format!("remove-{}", term.row.get())))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .accessibility_label(t("remove-word"))
                        .tooltip(t("remove-word"))
                        .on_click(Self::press(Input::TermRemoved(index), cx))
                });
                h_flex()
                    .gap_1()
                    .child(div().flex_1().child(TextInput::new(&term.input)))
                    .child(div().w(px(28.)).children(remove))
                    .into_any_element()
            })
            .collect();
        terms
    }

    /// Delete (an existing context's), Cancel and Save.
    fn editor_actions(is_new: bool, saving: bool, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .pt_2()
            .when(!is_new, |row| {
                row.child(
                    Button::new("delete-context")
                        .ghost()
                        .label(t("delete-ellipsis"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_delete(window, cx)),
                        ),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("cancel-context")
                    .label(t("cancel"))
                    .on_click(Self::press(Input::CancelEdit, cx)),
            )
            .child(
                Button::new("save-context")
                    .primary()
                    .label(if saving {
                        t("saving-progress")
                    } else {
                        t("save")
                    })
                    .loading(saving)
                    .disabled(saving)
                    .on_click(Self::press(Input::Save, cx)),
            )
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
            "a-context-tells-a-model-what-you",
            "cancel",
            "context-could-not-save",
            "context-edit",
            "context-name-placeholder",
            "context-pick-one",
            "context-prompt-description",
            "contexts",
            "contexts-add-a-word",
            "contexts-add-one",
            "contexts-could-not-load",
            "delete",
            "delete-context-body",
            "delete-context-title",
            "delete-ellipsis",
            "in-use",
            "loading-progress",
            "name",
            "names-jargon-anything-the-model-keeps-mishearing",
            "new-context",
            "no-context",
            "no-context-sends-nothing",
            "no-contexts-yet",
            "prompt",
            "remove-word",
            "save",
            "saving-progress",
            "use-context",
            "use-none",
            "words-to-listen-for",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
