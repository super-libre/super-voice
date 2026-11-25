// SPDX-License-Identifier: GPL-3.0-only
use cosmic::Element;
use cosmic::widget::{self, settings};
use super_voice_app_core::pages::settings::{
    IdleTime, Input, NOTIFICATION_METHODS, idle_time, idle_unload_choices,
};
use super_voice_app_core::state::settings::SettingsState;

use super::common::{error_banner, page_layout};
use crate::ui::messages::{LanguageMessage, Message, SettingsMessage};
use crate::ui::press_guard::press_guard;

fn page_input(input: Input) -> Message {
    Message::Settings(SettingsMessage::Page(input))
}

/// Settings page: what both features share — the primary language, how
/// failures are reported, and how long an unused model stays in memory. The
/// recording's sounds are speech to text's own, on its Behavior tab.
pub fn page<'a>(
    app_primary_language: Option<&'a str>,
    state: &SettingsState,
    action_error: Option<&'a str>,
) -> Element<'a, Message> {
    let lang_label = app_primary_language.map_or_else(
        || crate::fl!("automatic"),
        super_voice_app_core::languages::friendly_name,
    );
    let language_section = settings::section().title(crate::fl!("language")).add(
        settings::item::builder(crate::fl!("primary-language"))
            .description(crate::fl!("default-language-for-models-that-support-it"))
            .control(
                widget::button::standard(lang_label).on_press(Message::Language(
                    LanguageMessage::OpenLanguagePicker { model: None },
                )),
            ),
    );

    // Surface a failed save inline (Tier 1 #13) instead of letting the
    // failure flip the whole app to the connection-error page.
    let mut blocks: Vec<Element<'a, Message>> = Vec::new();
    if let Some(message) = action_error {
        blocks.push(error_banner(message));
    }
    blocks.push(language_section.into());
    blocks.push(notifications_section(state));
    blocks.push(memory_section(state.idle_unload_secs));

    page_layout(crate::fl!("settings"), settings::view_column(blocks))
}

/// Notifications section: how either feature's failures reach the user. One
/// setting for both, so it is here with the others they share.
fn notifications_section(state: &SettingsState) -> Element<'static, Message> {
    let method_names: Vec<String> = NOTIFICATION_METHODS
        .iter()
        .map(|m| crate::i18n::notification_method_name(*m))
        .collect();
    let selected_index = NOTIFICATION_METHODS
        .iter()
        .position(|m| *m == state.notification_method);

    settings::section()
        .title(crate::fl!("notifications"))
        .add(
            settings::item::builder(crate::fl!("failure-notices"))
                .description(crate::fl!("how-recording-and-speech-failures-are-reported"))
                .control(widget::dropdown(
                    method_names,
                    selected_index,
                    move |index| {
                        page_input(Input::NotificationMethodPicked(NOTIFICATION_METHODS[index]))
                    },
                )),
        )
        .into()
}

/// Memory section: whether a model sleeps when unused (is unloaded, and loads
/// again on its next use), and after how long. One setting for both features'
/// models.
fn memory_section(idle_unload_secs: u64) -> Element<'static, Message> {
    let enabled = idle_unload_secs > 0;
    let mut section = settings::section().title(crate::fl!("memory")).add(
        settings::item::builder(crate::fl!("sleep-when-idle"))
            .description(crate::fl!("sleep-when-idle-description"))
            .control(press_guard(
                cosmic::widget::toggler(enabled)
                    .on_toggle(|on| page_input(Input::SleepToggled(on))),
            )),
    );

    if enabled {
        let choices = idle_unload_choices(idle_unload_secs);
        let names: Vec<String> = choices.iter().map(|secs| idle_time_name(*secs)).collect();
        let selected = choices.iter().position(|secs| *secs == idle_unload_secs);
        section = section.add(settings::item::builder(crate::fl!("sleep-after")).control(
            widget::dropdown(names, selected, move |index| {
                page_input(Input::IdleUnloadPicked(choices[index]))
            }),
        ));
    }

    section.into()
}

/// An idle time as the dropdown shows it: whole minutes when it is one, else
/// seconds.
pub(crate) fn idle_time_name(secs: u64) -> String {
    match idle_time(secs) {
        IdleTime::Minutes(minutes) => crate::fl!("idle-minutes", minutes = minutes),
        IdleTime::Seconds(seconds) => crate::fl!("idle-seconds", seconds = seconds),
    }
}
