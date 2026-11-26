// SPDX-License-Identifier: GPL-3.0-only
//! A backend's Configure sheet, from a step's card: the dictation context it
//! is sent (for a backend that takes one), then what it requires (an API
//! key), then its options. A right `Sheet`, at every width: the window's
//! whole width when it is narrow.
//!
//! Secrets never read back: a stored one says Configured, with Remove. A
//! switch or a pick from a list saves as it changes; a text field saves on
//! Save or Enter, and Reset takes it back to the backend's default. The logic
//! is the core's `pages::configure`.

use std::collections::HashMap;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _,
    Pixels, Render, SharedString, Styled as _, Subscription, WeakEntity, Window, div, px,
};
use super_voice_app_core::daemon::backends::{BackendInfo, BackendOption, BackendSecret};
use super_voice_app_core::daemon::client::list_backends;
use super_voice_app_core::daemon::client::v1::contexts::{ContextCatalog, list_contexts};
use super_voice_app_core::pages::configure::{
    self as sheet, ConfigureState, ContextPick, Effect, Input as SheetInput, pick_of, takes_context,
};
use super_voice_shared::models::contexts::DictationContext;

use super::pickers::{Choice, PickerKey};
use super::{In, ModelsPage};
use crate::effects::{CorePage as _, Runner, runner};
use crate::icons::Phosphor;
use crate::ui::settings_row::SettingsRow;
use crate::ui::width::{self, WindowSize};
use crate::{t, t_with};

/// The picker's ids for following the active context and for none; any
/// other is a context's own.
const FOLLOW: &str = "\u{0}follow";
const NOTHING: &str = "\u{0}none";

/// A text field of the sheet: a secret, or a text option.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Field {
    Secret(String, String),
    Option(String, String),
}

/// What the sheet holds beside the core's state.
#[derive(Default)]
pub struct Sheet {
    pub state: ConfigureState,
    /// Every installed backend, which the core reads options and defaults
    /// from.
    pub backends: Vec<BackendInfo>,
    pub contexts: ContextCatalog,
    /// The backend list was read since the sheet opened.
    pub loaded: bool,
    /// The backend the sheet is reading the list for, to open over it.
    pub reading: Option<String>,
    fields: HashMap<Field, (Entity<InputState>, Subscription)>,
}

/// The sheet's content: the page's, drawn by the page.
struct ConfigureView {
    page: WeakEntity<ModelsPage>,
    _changed: Subscription,
}

impl Render for ConfigureView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page.upgrade().map_or_else(
            || div().into_any_element(),
            |page| page.update(cx, |page, cx| page.configure_body(window, cx)),
        )
    }
}

/// The context picked by the picker row `id`.
pub fn context_pick(id: &str) -> ContextPick {
    match id {
        FOLLOW => ContextPick::FollowActive,
        NOTHING => ContextPick::Nothing,
        id => ContextPick::Pinned(id.to_string()),
    }
}

/// How wide the sheet is: the window's width when it is narrow.
fn sheet_width(window: &Window) -> Pixels {
    match WindowSize::of(window) {
        WindowSize::Wide => px(460.),
        WindowSize::Compact => px(400.),
        WindowSize::Narrow => width::window_width(window),
    }
}

impl ModelsPage {
    /// Open the Configure sheet over `source`.
    pub(super) fn open_configure(
        &mut self,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pickers.forget_sheet();
        self.sheet.fields.clear();
        self.read_sheet(source, window, cx);
        let page = cx.weak_entity();
        let Some(entity) = page.upgrade() else {
            return;
        };
        let view = cx.new(|cx| ConfigureView {
            page: page.clone(),
            _changed: cx.observe(&entity, |_, _, cx| cx.notify()),
        });
        window.open_sheet(cx, move |sheet, window, cx| {
            let title = page
                .upgrade()
                .map_or_else(SharedString::default, |page| page.read(cx).sheet_title());
            let on_done = page.clone();
            let on_close = page.clone();
            sheet
                .size(sheet_width(window))
                .title(title)
                .child(view.clone())
                .footer(
                    h_flex().w_full().justify_end().child(
                        Button::new("configure-done")
                            .primary()
                            .label(t("done"))
                            .on_click(move |_, window, cx| {
                                window.close_sheet(cx);
                                _ = on_done
                                    .update(cx, |page, cx| page.configure_closed(window, cx));
                            }),
                    ),
                )
                .on_close(move |_, window, cx| {
                    _ = on_close.update(cx, |page, cx| page.configure_closed(window, cx));
                })
        });
    }

    /// Read the backend list, then open the sheet's state over `source`.
    pub(super) fn read_sheet(
        &mut self,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet.loaded = false;
        self.sheet.reading = Some(source.clone());
        let work = runner(async move {
            let result = list_backends().await.map_err(|e| e.to_string());
            In::SheetOpened { source, result }
        });
        crate::effects::run(work, window, cx).detach();
    }

    fn configure_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet.state.open.is_none() && self.sheet.reading.is_none() {
            return;
        }
        self.sheet.reading = None;
        self.send(In::Configure(SheetInput::Close), window, cx);
    }

    /// The sheet's title: "Whisper configuration".
    fn sheet_title(&self) -> SharedString {
        let source = self
            .sheet
            .state
            .open
            .as_deref()
            .or(self.sheet.reading.as_deref())
            .unwrap_or_default();
        let name = self
            .sheet
            .backends
            .iter()
            .find(|b| b.source == source)
            .map(|b| b.name.clone())
            .or_else(|| {
                self.stages().into_iter().find_map(|stage| {
                    let view = self.models.view(stage);
                    (view.source.as_deref() == Some(source)).then(|| view.name.clone())?
                })
            })
            .unwrap_or_else(|| source.to_string());
        t_with("backend-configuration", &[("name", &name)])
    }

    /// Do a sheet effect now, outside of an update.
    pub(super) fn run_sheet_effect(
        &mut self,
        effect: Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(work) = self.sheet_effect(effect, window, cx) {
            crate::effects::run(work, window, cx).detach();
        }
    }

    pub(super) fn sheet_effect(
        &mut self,
        effect: Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<In>> {
        let configure = |work: Runner<SheetInput>| -> Option<Runner<In>> {
            Some(runner(async move { In::Configure(work.await) }))
        };
        match effect {
            // Only the open backend's are shown.
            Effect::ListSecrets(source) => {
                if self.sheet.state.open.as_ref() != Some(&source) {
                    return None;
                }
                configure(runner(sheet::run_list_secrets(source)))
            }
            Effect::SaveSecret {
                source,
                name,
                value,
            } => configure(runner(sheet::run_save_secret(source, name, value))),
            Effect::RemoveSecret { source, name } => {
                configure(runner(sheet::run_remove_secret(source, name)))
            }
            Effect::SetOption {
                source,
                name,
                value,
            } => configure(runner(sheet::run_set_option(source, name, value))),
            Effect::ClearOption { source, name } => {
                configure(runner(sheet::run_clear_option(source, name)))
            }
            Effect::ReadContext(source) => configure(runner(sheet::run_read_context(source))),
            Effect::PickContext { source, pick } => {
                configure(runner(sheet::run_pick_context(source, pick)))
            }
            // A key or option written: whether each step is set up changes
            // with it.
            Effect::ReloadBackends => {
                self.send(
                    In::Models(super_voice_app_core::pages::models::Input::ReloadBackends),
                    window,
                    cx,
                );
                Some(runner(async move {
                    In::Backends(list_backends().await.map_err(|e| e.to_string()))
                }))
            }
            Effect::ReloadContexts => {
                Some(runner(
                    async move { In::Contexts(list_contexts().await.ok()) },
                ))
            }
        }
    }

    /// The text field `field`, built the first time it is drawn, holding what
    /// the core says is typed in it.
    fn field(
        &mut self,
        field: &Field,
        masked: bool,
        typed: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some((state, _)) = self.sheet.fields.get(field) {
            let state = state.clone();
            if state.read(cx).value().as_ref() != typed {
                let typed = typed.to_string();
                state.update(cx, |state, cx| state.set_value(typed, window, cx));
            }
            return state;
        }
        let placeholder = if masked {
            t("enter-api-key")
        } else {
            SharedString::default()
        };
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(masked)
                .placeholder(placeholder)
                .default_value(typed.to_string())
        });
        let key = field.clone();
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |page, state, event: &InputEvent, window, cx| {
                let (Field::Secret(source, name) | Field::Option(source, name)) = key.clone();
                let input = match (event, &key) {
                    (InputEvent::Change, Field::Secret(..)) => SheetInput::SecretTyped {
                        source,
                        name,
                        value: state.read(cx).value().to_string(),
                    },
                    (InputEvent::Change, Field::Option(..)) => SheetInput::OptionTyped {
                        source,
                        name,
                        value: state.read(cx).value().to_string(),
                    },
                    (InputEvent::PressEnter { .. }, Field::Secret(..)) => {
                        SheetInput::SaveSecret { source, name }
                    }
                    (InputEvent::PressEnter { .. }, Field::Option(..)) => {
                        SheetInput::SaveOption { source, name }
                    }
                    _ => return,
                };
                page.send(In::Configure(input), window, cx);
            },
        );
        self.sheet
            .fields
            .insert(field.clone(), (state.clone(), subscription));
        state
    }

    /// The sheet's body.
    fn configure_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let Some(source) = self.sheet.state.open.clone().filter(|_| self.sheet.loaded) else {
            return div()
                .py_4()
                .text_sm()
                .text_color(muted)
                .child(t("loading-progress"))
                .into_any_element();
        };
        let Some(backend) = self
            .sheet
            .backends
            .iter()
            .find(|b| b.source == source)
            .cloned()
        else {
            return div()
                .py_4()
                .text_sm()
                .text_color(muted)
                .child(t("this-backend-has-nothing-to-configure"))
                .into_any_element();
        };
        // The rows' width: the sheet's, less its sides.
        let row = sheet_width(window) - px(32.);
        let mut groups: Vec<AnyElement> = Vec::new();

        if takes_context(&self.sheet.backends, &source) {
            groups.push(self.context_group(&source, window, cx));
        }

        let mut required: Vec<AnyElement> = Vec::new();
        let mut optional: Vec<AnyElement> = Vec::new();
        for secret in &backend.secrets {
            let row = self.secret_row(&source, secret, row, window, cx);
            if secret.required {
                required.push(row);
            } else {
                optional.push(row);
            }
        }
        for option in &backend.options {
            let row = self.option_row(&source, option, row, window, cx);
            if option.required {
                required.push(row);
            } else {
                optional.push(row);
            }
        }
        if !required.is_empty() {
            groups.push(group(t("configure-required"), required));
        }
        if !optional.is_empty() {
            groups.push(group(t("configure-options"), optional));
        }
        if groups.is_empty() {
            groups.push(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(t("this-backend-has-nothing-to-configure"))
                    .into_any_element(),
            );
        }
        v_flex().gap_5().py_2().children(groups).into_any_element()
    }

    /// Which dictation context the backend is sent.
    fn context_group(
        &mut self,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let contexts = &self.sheet.contexts;
        let follow = match contexts.active.as_deref() {
            Some(id) => t_with(
                "follow-active-context",
                &[(
                    "value0",
                    &contexts
                        .contexts
                        .iter()
                        .find(|c| c.id == id)
                        .map_or_else(|| id.to_string(), DictationContext::display_name),
                )],
            ),
            None => t("follow-the-active-context-none-right-now"),
        };
        let mut items = vec![
            Choice::new(FOLLOW, follow),
            Choice::new(NOTHING, t("no-context")),
        ];
        items.extend(
            contexts
                .contexts
                .iter()
                .map(|c| Choice::new(c.id.clone(), c.display_name())),
        );
        let ids: Vec<String> = contexts.contexts.iter().map(|c| c.id.clone()).collect();
        let current = self.sheet.state.backend_contexts.get(source);
        let selected = match pick_of(current, &ids) {
            ContextPick::FollowActive => FOLLOW.to_string(),
            ContextPick::Nothing => NOTHING.to_string(),
            ContextPick::Pinned(id) => id,
        };
        let hint = current.and_then(|c| c.context.as_ref()).map_or_else(
            || t("nothing-is-sent-to-this-backend-right"),
            |context| t_with("sending-context", &[("value0", &context.display_name())]),
        );
        let state = self.pickers.get(
            &PickerKey::Context,
            items,
            Some(&selected),
            false,
            window,
            cx,
        );
        group(
            t("dictation-context"),
            vec![
                v_flex()
                    .gap_2()
                    .py_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(hint),
                    )
                    .child(
                        Select::new(&state)
                            .w_full()
                            .accessibility_label(t("dictation-context")),
                    )
                    .into_any_element(),
            ],
        )
    }

    fn secret_row(
        &mut self,
        source: &str,
        secret: &BackendSecret,
        row: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = label(
            secret.label.as_deref().unwrap_or(&secret.name),
            secret.required,
        );
        let hint = (!secret.description.is_empty()).then(|| secret.description.clone());
        let (source, name) = (source.to_string(), secret.name.clone());
        if self.sheet.state.secret_configured(&source, &name) {
            let remove = cx.listener(move |page, _, window, cx| {
                page.send(
                    In::Configure(SheetInput::RemoveSecret {
                        source: source.clone(),
                        name: name.clone(),
                    }),
                    window,
                    cx,
                );
            });
            return SettingsRow::new(label)
                .when_some(hint, SettingsRow::description)
                .width(row)
                .inline()
                .control(
                    h_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_1()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    gpui_kit::component::Icon::new(Phosphor::Check)
                                        .size_4()
                                        .text_color(cx.theme().success),
                                )
                                .child(t("configured")),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-{}", secret.name)))
                                .ghost()
                                .small()
                                .label(t("remove"))
                                .on_click(remove),
                        ),
                )
                .into_any_element();
        }
        let typed = self.sheet.state.secret_input(&source, &name).to_string();
        let state = self.field(
            &Field::Secret(source.clone(), name.clone()),
            true,
            &typed,
            window,
            cx,
        );
        let save = cx.listener(move |page, _, window, cx| {
            page.send(
                In::Configure(SheetInput::SaveSecret {
                    source: source.clone(),
                    name: name.clone(),
                }),
                window,
                cx,
            );
        });
        stacked(
            label,
            hint,
            h_flex()
                .gap_2()
                .child(Input::new(&state).mask_toggle().flex_1())
                .child(
                    Button::new(SharedString::from(format!("save-{}", secret.name)))
                        .primary()
                        .label(t("save"))
                        .disabled(typed.is_empty())
                        .on_click(save),
                ),
            cx,
        )
    }

    /// A switch: the flip is the save.
    fn switch_row(
        source: String,
        option: &BackendOption,
        label: SharedString,
        description: Option<String>,
        row: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = option.name.clone();
        let toggle = cx.listener(move |page, on: &bool, window, cx| {
            page.send(
                In::Configure(SheetInput::ToggleOption {
                    source: source.clone(),
                    name: name.clone(),
                    value: *on,
                }),
                window,
                cx,
            );
        });
        SettingsRow::new(label.clone())
            .when_some(description, SettingsRow::description)
            .width(row)
            .inline()
            .control(
                Switch::new(SharedString::from(format!("option-{}", option.name)))
                    .checked(option.bool_value())
                    .accessibility_label(label)
                    .on_change(toggle),
            )
            .into_any_element()
    }

    /// A pick from a list: picking is the save.
    #[allow(
        clippy::too_many_arguments,
        reason = "a row's parts, as its siblings take them"
    )]
    fn choice_row(
        &mut self,
        source: String,
        option: &BackendOption,
        label: SharedString,
        description: Option<String>,
        row: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let items: Vec<Choice> = option
            .choices
            .iter()
            .map(|c| Choice::new(c.clone(), c.clone()))
            .collect();
        let selected = option.choice_index().map(|i| option.choices[i].clone());
        let picker = self.pickers.get(
            &PickerKey::Option(source, option.name.clone()),
            items,
            selected.as_deref(),
            false,
            window,
            cx,
        );
        SettingsRow::new(label.clone())
            .when_some(description, SettingsRow::description)
            .width(row)
            .inline()
            .control(Select::new(&picker).w(px(160.)).accessibility_label(label))
            .into_any_element()
    }

    fn option_row(
        &mut self,
        source: &str,
        option: &BackendOption,
        row: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = label(
            option.label.as_deref().unwrap_or(&option.name),
            option.required,
        );
        let description = (!option.description.is_empty()).then(|| option.description.clone());
        let (source, name) = (source.to_string(), option.name.clone());
        if option.is_bool() {
            return Self::switch_row(source, option, label, description, row, cx);
        }
        if option.has_choices() {
            return self.choice_row(source, option, label, description, row, window, cx);
        }
        let typed = self.sheet.state.option_input(&source, &name).to_string();
        let state = self.field(
            &Field::Option(source.clone(), name.clone()),
            false,
            &typed,
            window,
            cx,
        );
        let overridden = option.value.as_deref() != option.default.as_deref();
        let changed = option.value.as_deref().unwrap_or_default() != typed;
        let (save_source, save_name) = (source.clone(), name.clone());
        let save = cx.listener(move |page, _, window, cx| {
            page.send(
                In::Configure(SheetInput::SaveOption {
                    source: save_source.clone(),
                    name: save_name.clone(),
                }),
                window,
                cx,
            );
        });
        let reset = cx.listener(move |page, _, window, cx| {
            page.send(
                In::Configure(SheetInput::ResetOption {
                    source: source.clone(),
                    name: name.clone(),
                }),
                window,
                cx,
            );
        });
        stacked(
            label,
            text_hint(option),
            h_flex()
                .gap_2()
                .child(Input::new(&state).flex_1())
                .child(
                    Button::new(SharedString::from(format!("save-{}", option.name)))
                        .label(t("save"))
                        .disabled(!changed)
                        .on_click(save),
                )
                .when(overridden, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("reset-{}", option.name)))
                            .ghost()
                            .label(t("reset"))
                            .on_click(reset),
                    )
                }),
            cx,
        )
    }
}

/// A row's title, with a mark when it is required.
fn label(label: &str, required: bool) -> SharedString {
    if required {
        format!("{label} *").into()
    } else {
        label.to_string().into()
    }
}

/// A text option's description, with its default when it has one.
fn text_hint(option: &BackendOption) -> Option<SharedString> {
    let default = option.default.as_deref().filter(|d| !d.is_empty());
    match (option.description.as_str(), default) {
        ("", None) => None,
        (hint, None) => Some(hint.to_string().into()),
        ("", Some(default)) => Some(t_with("setting-default", &[("default", default)])),
        (hint, Some(default)) => Some(t_with(
            "setting-hint-default",
            &[("hint", hint), ("default", default)],
        )),
    }
}

/// A row whose control is long, under its text: a text field and its buttons.
fn stacked(
    title: SharedString,
    hint: Option<impl Into<SharedString>>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    v_flex()
        .w_full()
        .gap_2()
        .py_3()
        .child(
            v_flex()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .when_some(hint, |text, hint| {
                    text.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(hint.into()),
                    )
                }),
        )
        .child(control)
        .into_any_element()
}

/// A titled group of rows, as the kit draws one.
fn group(title: SharedString, rows: Vec<AnyElement>) -> AnyElement {
    GroupBox::new()
        .outline()
        .title(title)
        .content_style(gpui_kit::StyleRefinement::default().py_0().gap_0())
        .children(rows)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_rows_read_back() {
        assert_eq!(context_pick(FOLLOW), ContextPick::FollowActive);
        assert_eq!(context_pick(NOTHING), ContextPick::Nothing);
        assert_eq!(context_pick("coding"), ContextPick::Pinned("coding".into()));
    }
}
