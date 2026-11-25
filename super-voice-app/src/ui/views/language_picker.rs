// SPDX-License-Identifier: GPL-3.0-only
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget::{self, button, column, scrollable, text_input};

use crate::core::app::AppModel;
use crate::ui::messages::{LanguageMessage, Message, SettingsMessage};
use super_voice_app_core::languages::picker_rows;
use super_voice_app_core::pages::model_language::{self, ModelKey};
use super_voice_app_core::pages::settings::Input;

pub fn sheet(app: &AppModel) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let query = &app.language.language_picker_query;

    // The special entries stay pinned at the top, whatever the search; the
    // languages under them are sorted by name and narrowed by it.
    let mut pinned: Vec<(Option<String>, String)> = Vec::new();
    let offers: &[String] =
        if let Some((stage, ref src, ref mdl)) = app.language.language_picker_target {
            // Per-model sheet.
            pinned.push((None, crate::fl!("follow-global"))); // clear → DELETE
            // What the daemon will accept for this model, from
            // `/language/list` — `auto` included, which is why it is not pinned
            // here the way the global sheet pins it. Offering a general BCP-47 list
            // instead would put tags in front of the user that the setter refuses,
            // discoverable only by choosing one.
            app.language
                .model_languages
                .offered(stage, src, mdl)
                .unwrap_or_default()
        } else {
            // Global sheet — the daemon's own vocabulary, `auto` included, from
            // `/settings/language/list`. The unset state is reached by not choosing
            // anything, so there is no explicit "No preference" entry.
            &app.language.global_offers
        };
    let rows: Vec<(Option<String>, String)> = pinned
        .into_iter()
        .chain(
            picker_rows(offers, query)
                .into_iter()
                .map(|(tag, name)| (Some(tag), name)),
        )
        .collect();

    // Search field pinned at the top.
    let search = text_input(
        crate::fl!("search-languages-progress"),
        &app.language.language_picker_query,
    )
    .on_input(|s| Message::Language(LanguageMessage::LanguagePickerQueryChanged(s)))
    .width(Length::Fill);

    let mut list = column::with_capacity(rows.len()).spacing(spacing.space_xxs);
    for (tag, label) in rows {
        let msg = if let Some((stage, ref src, ref mdl)) = app.language.language_picker_target {
            Message::Language(LanguageMessage::Model(model_language::Input::Picked(
                ModelKey::new(stage, src.clone(), mdl.clone()),
                tag,
            )))
        } else {
            Message::Settings(SettingsMessage::Page(Input::PrimaryLanguagePicked(tag)))
        };
        list = list.push(button::text(label).width(Length::Fill).on_press(msg));
    }

    widget::column::with_capacity(2)
        .spacing(spacing.space_s)
        .push(search)
        .push(scrollable(list).height(Length::Fill))
        .width(Length::Fill)
        .into()
}
