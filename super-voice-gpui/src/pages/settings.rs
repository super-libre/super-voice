// SPDX-License-Identifier: GPL-3.0-only
//! The Settings page: what both features share. The primary language, how
//! recording and speech failures are reported, and whether an unused model
//! leaves memory, and after how long.
//!
//! The logic is the core's (`pages::settings`): the failure notices and
//! Sleep When Idle move once the daemon has stored them, the primary
//! language at once. Each control is a kit `Select` or `Switch` in a settings
//! row, so a narrow window puts it under its words.

use gpui_kit::component::IndexPath;
use gpui_kit::component::select::{
    SearchableVec, Select, SelectDelegate, SelectEvent, SelectItem, SelectState,
};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, div, px,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::languages;
use super_voice_app_core::pages::settings::{
    self as page, Effect, IdleTime, Input, NOTIFICATION_METHODS, Notice,
};
use super_voice_app_core::state::language::LanguageState;
use super_voice_app_core::state::settings::SettingsState;
use super_voice_shared::models::notification_method::NotificationMethod;

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView};
use crate::shared::Shared;
use crate::ui::card::Card;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, t_with};

/// A select's width beside its words.
const SELECT_WIDTH: Pixels = px(220.);
/// A short select's width, which stays beside its words in a narrow window.
const SHORT_SELECT_WIDTH: Pixels = px(150.);

/// One choice of a select: its value and its words.
#[derive(Clone)]
struct Choice<V> {
    value: V,
    title: SharedString,
}

impl<V: Clone + PartialEq> SelectItem for Choice<V> {
    type Value = V;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &V {
        &self.value
    }
}

/// A select over the core's values, and the choices it was last given.
struct Picker<V: Clone + PartialEq + 'static> {
    state: Entity<SelectState<SearchableVec<Choice<V>>>>,
    shown: Vec<(V, SharedString)>,
}

impl<V: Clone + PartialEq + 'static> Picker<V> {
    fn new(searchable: bool, window: &mut Window, cx: &mut Context<SettingsPage>) -> Self {
        Self {
            state: cx.new(|cx| {
                SelectState::new(SearchableVec::new(Vec::new()), None, window, cx)
                    .searchable(searchable)
            }),
            shown: Vec::new(),
        }
    }

    /// Offer `choices` with `selected` picked, touching the select only
    /// where it differs.
    fn show(
        &mut self,
        choices: Vec<(V, SharedString)>,
        selected: &V,
        window: &mut Window,
        cx: &mut Context<SettingsPage>,
    ) {
        if choices != self.shown {
            let items: Vec<Choice<V>> = choices
                .iter()
                .map(|(value, title)| Choice {
                    value: value.clone(),
                    title: title.clone(),
                })
                .collect();
            self.state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(items), window, cx);
            });
            self.shown = choices;
        }
        if self.state.read(cx).selected_value() != Some(selected) {
            let index = self
                .shown
                .iter()
                .position(|(value, _)| value == selected)
                .map(|row| IndexPath::default().row(row));
            self.state.update(cx, |state, cx| {
                state.set_selected_index(index, window, cx);
            });
        }
    }
}

pub struct SettingsPage {
    state: SettingsState,
    language: LanguageState,
    primary_language: Picker<Option<String>>,
    notification_method: Picker<NotificationMethod>,
    idle_unload: Picker<u64>,
    _subscriptions: Vec<Subscription>,
}

impl PageView for SettingsPage {
    fn new(_: Page, _: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let primary_language: Picker<Option<String>> = Picker::new(true, window, cx);
        let notification_method: Picker<NotificationMethod> = Picker::new(false, window, cx);
        let idle_unload: Picker<u64> = Picker::new(false, window, cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &primary_language.state,
                window,
                |this, _, event: &SelectEvent<_>, window, cx| {
                    let SelectEvent::Confirm(Some(tag)) = event else {
                        return;
                    };
                    if *tag != this.language.primary_language {
                        this.send(Input::PrimaryLanguagePicked(tag.clone()), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &notification_method.state,
                window,
                |this, _, event: &SelectEvent<_>, window, cx| {
                    if let SelectEvent::Confirm(Some(method)) = event
                        && *method != this.state.notification_method
                    {
                        this.send(Input::NotificationMethodPicked(*method), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &idle_unload.state,
                window,
                |this, _, event: &SelectEvent<_>, window, cx| {
                    if let SelectEvent::Confirm(Some(after_secs)) = event
                        && *after_secs != this.state.idle_unload_secs
                    {
                        this.send(Input::IdleUnloadPicked(*after_secs), window, cx);
                    }
                },
            ),
        ];
        let mut page = Self {
            state: SettingsState::default(),
            language: LanguageState::default(),
            primary_language,
            notification_method,
            idle_unload,
            _subscriptions: subscriptions,
        };
        page.sync(window, cx);
        page
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(Input::Refresh, window, cx);
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = page::heard(event) {
            self.send(input, window, cx);
        }
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        self.language.global_offers = ["de", "en", "es", "fr", "it", "ja", "pt", "zh"]
            .map(String::from)
            .to_vec();
        self.language.primary_language = None;
        self.state.notification_method = NotificationMethod::Auto;
        self.state.idle_unload_secs = if scenes.contains(&"awake") { 0 } else { 600 };
        self.sync(window, cx);
    }
}

impl CorePage for SettingsPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, &mut self.language, input);
        effects::run_effects(self, outcome.effects, window, cx);
        toast::notice::<Self>(
            window,
            cx,
            outcome.clear_notice,
            outcome.notice.map(|notice| {
                let (key, error) = match notice {
                    Notice::CouldNotSaveNotificationMethod(e) => {
                        ("notification-method-could-not-save", e)
                    }
                    Notice::CouldNotSaveIdleUnload(e) => ("idle-unload-could-not-save", e),
                    Notice::CouldNotSaveLanguage(e) => ("language-could-not-save", e),
                };
                Toast::error(t_with(key, &[("error", &error)]))
            }),
        );
        self.sync(window, cx);
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        Some(match effect {
            Effect::ReadNotificationMethod => runner(page::run_load_notification_method()),
            Effect::ReadIdleUnload => runner(page::run_load_idle_unload()),
            Effect::ReadPrimaryLanguage => runner(page::run_load_primary_language()),
            Effect::ReadLanguages => runner(page::run_load_languages()),
            Effect::SaveNotificationMethod(method) => {
                runner(page::run_save_notification_method(method))
            }
            Effect::SaveIdleUnload(after_secs) => runner(page::run_save_idle_unload(after_secs)),
            Effect::SavePrimaryLanguage(tag) => runner(page::run_save_primary_language(tag)),
            // What follows the language (a model's) hears the daemon say it
            // changed, and reads it again itself.
            Effect::LanguageChanged => return None,
        })
    }
}

impl SettingsPage {
    /// Put the core's values in the selects.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let primary = self.language.primary_language.clone();
        self.primary_language
            .show(language_choices(&self.language), &primary, window, cx);
        let method = self.state.notification_method;
        self.notification_method.show(
            NOTIFICATION_METHODS
                .iter()
                .map(|method| (*method, notification_method_name(*method)))
                .collect(),
            &method,
            window,
            cx,
        );
        let after_secs = self.state.idle_unload_secs;
        self.idle_unload.show(
            page::idle_unload_choices(after_secs)
                .into_iter()
                .map(|secs| (secs, idle_time_name(secs)))
                .collect(),
            &after_secs,
            window,
            cx,
        );
    }

    fn language(&self, stacked: bool) -> Card {
        Card::new().title(t("language")).child(
            SettingsRow::new(t("primary-language"))
                .description(t("default-language-for-models-that-support-it"))
                .control(sized(
                    Select::new(&self.primary_language.state)
                        .search_placeholder(t("search-languages-progress"))
                        .accessibility_label(t("primary-language")),
                    stacked,
                )),
        )
    }

    fn notifications(&self, stacked: bool) -> Card {
        Card::new().title(t("notifications")).child(
            SettingsRow::new(t("failure-notices"))
                .description(t("how-recording-and-speech-failures-are-reported"))
                .control(sized(
                    Select::new(&self.notification_method.state)
                        .accessibility_label(t("failure-notices")),
                    stacked,
                )),
        )
    }

    fn memory(&self, cx: &mut Context<Self>) -> Card {
        let sleep = self.state.idle_unload_secs > 0;
        Card::new().title(t("memory")).child(
            v_flex()
                .child(
                    SettingsRow::new(t("sleep-when-idle"))
                        .description(t("sleep-when-idle-description"))
                        .inline()
                        .control(
                            Switch::new("sleep-when-idle")
                                .checked(sleep)
                                .accessibility_label(t("sleep-when-idle"))
                                .on_change(cx.listener(|this, on: &bool, window, cx| {
                                    this.send(Input::SleepToggled(*on), window, cx);
                                })),
                        ),
                )
                .when(sleep, |rows| {
                    rows.child(Separator::horizontal()).child(
                        SettingsRow::new(t("sleep-after")).inline().control(
                            Select::new(&self.idle_unload.state)
                                .w(SHORT_SELECT_WIDTH)
                                .accessibility_label(t("sleep-after")),
                        ),
                    )
                }),
        )
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        // A row narrower than 480 px puts its control under its words, at
        // the row's width.
        let stacked = width::row_stacks(width::content(window) - padding * 2.);
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::Settings))
            .child(
                div()
                    .id("settings")
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
                            .child(self.language(stacked))
                            .child(self.notifications(stacked))
                            .child(self.memory(cx)),
                    ),
            )
    }
}

/// A select at the row's width when the row puts it under its words,
/// otherwise beside them at its own.
fn sized<D: SelectDelegate + 'static>(select: Select<D>, stacked: bool) -> Select<D>
where
    <D::Item as SelectItem>::Value: PartialEq + Clone,
{
    if stacked {
        select.w_full()
    } else {
        select.w(SELECT_WIDTH)
    }
}

/// The primary language's choices: Automatic, then every language the
/// daemon offers by its name, and the one in force if it offers it no more.
fn language_choices(language: &LanguageState) -> Vec<(Option<String>, SharedString)> {
    let mut rows: Vec<(String, String)> = languages::picker_rows(&language.global_offers, "");
    if let Some(tag) = &language.primary_language
        && !rows.iter().any(|(offered, _)| offered == tag)
    {
        rows.push((tag.clone(), languages::friendly_name(tag)));
    }
    std::iter::once((None, t("automatic")))
        .chain(rows.into_iter().map(|(tag, name)| (Some(tag), name.into())))
        .collect()
}

/// A way failures are reported, in words.
fn notification_method_name(method: NotificationMethod) -> SharedString {
    t(match method {
        NotificationMethod::Auto => "auto-recommended",
        NotificationMethod::Desktop => "desktop-notification",
        NotificationMethod::Typed => "type-into-window",
        NotificationMethod::Off => "off",
    })
}

/// An idle time, in words.
fn idle_time_name(after_secs: u64) -> SharedString {
    let (key, arg, n) = match page::idle_time(after_secs) {
        IdleTime::Minutes(n) => ("idle-minutes", "minutes", n),
        IdleTime::Seconds(n) => ("idle-seconds", "seconds", n),
    };
    t_with(key, &[(arg, &n.to_string())])
}

#[cfg(test)]
mod tests {
    use super_voice_app_core::state::language::LanguageState;

    #[test]
    fn idle_times_read_in_words() {
        assert_eq!(super::idle_time_name(300).as_ref(), "5 minutes");
        assert_eq!(super::idle_time_name(90).as_ref(), "90 seconds");
    }

    /// Automatic comes first, and a language the daemon no longer offers
    /// stays a choice while it is the one in force.
    #[test]
    fn the_language_in_force_is_always_a_choice() {
        let language = LanguageState {
            global_offers: vec!["fr".into(), "de".into()],
            primary_language: Some("pt".into()),
            ..LanguageState::default()
        };
        let choices = super::language_choices(&language);
        assert_eq!(choices[0].0, None);
        assert!(choices.iter().any(|(tag, _)| tag.as_deref() == Some("pt")));
        assert_eq!(choices.len(), 4);
    }

    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "auto-recommended",
            "automatic",
            "default-language-for-models-that-support-it",
            "desktop-notification",
            "failure-notices",
            "how-recording-and-speech-failures-are-reported",
            "idle-minutes",
            "idle-seconds",
            "idle-unload-could-not-save",
            "language",
            "language-could-not-save",
            "memory",
            "notification-method-could-not-save",
            "notifications",
            "off",
            "primary-language",
            "search-languages-progress",
            "sleep-after",
            "sleep-when-idle",
            "sleep-when-idle-description",
            "type-into-window",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
