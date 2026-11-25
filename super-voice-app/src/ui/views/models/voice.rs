// SPDX-License-Identifier: GPL-3.0-only
//! The synthesis card's voice control: which voice the model speaks in, and
//! the one warning that says it has none.

use cosmic::Element;
use cosmic::iced::Alignment;
use cosmic::iced::widget::row;
use cosmic::widget::{self, text};

use crate::core::app::AppModel;
use crate::daemon::backends::BackendInfo;
use crate::ui::icons;
use crate::ui::messages::{Message, VoiceMessage};
use super_voice_app_core::pages::model_voice::{self, DefaultVoice, Input, VoiceWarning};

/// The voice dropdown for `model`, or `None` when there is nothing to pick.
///
/// Row 0 is always the model's own default, so clearing the preference is a
/// pick like any other rather than a second control. A model that declares no
/// `default_voice` — every cloning model — labels that row as the state it
/// really is: nothing chosen, and speech refused until something is.
///
/// Returns `None` when there is nothing to pick from: a model whose voices are
/// all described takes free text and has no list, and one that clones before
/// any clip has been recorded has an empty library. The caller skips the push
/// rather than rendering an empty control.
pub(super) fn voice_dropdown<'a>(
    backend: &'a BackendInfo,
    model: &str,
    app: &'a AppModel,
) -> Option<Element<'a, Message>> {
    // Stale-block guard, the same one the language button uses: the voice
    // state describes one model, and a card drawing another must not read it.
    if !model_voice::shows(&app.voice, &backend.source, model) {
        return None;
    }
    let (default, voices) = model_voice::picker(&app.voice)?;

    let default_row = match default {
        DefaultVoice::NoneChosen => crate::fl!("no-cloned-voice-chosen"),
        DefaultVoice::Named(label) => crate::fl!("voice-default", label = label),
    };
    let mut labels: Vec<String> = vec![default_row];
    labels.extend(voices.iter().map(|(v, cloned)| {
        if *cloned {
            crate::fl!("voice-cloned", label = v.label.as_str())
        } else {
            v.label.clone()
        }
    }));

    // The selected row is the *stored* voice, not the effective one: with no
    // preference set the default row is what is selected, which is exactly what
    // `override: null` means.
    let selected = model_voice::chosen(&app.voice)
        .and_then(|id| voices.iter().position(|(v, _)| v.id == id).map(|i| i + 1));

    // The ids are cloned into the closure rather than indexed out of `app` on
    // press: the catalog can be replaced by a refresh between render and click,
    // and an index into a list that has since changed would store the wrong
    // voice.
    let ids: Vec<String> = voices.iter().map(|(v, _)| v.id.clone()).collect();
    Some(
        widget::dropdown(labels, selected.or(Some(0)), move |index| {
            Message::Voice(VoiceMessage::Page(Input::Picked(
                index.checked_sub(1).map(|i| ids[i].clone()),
            )))
        })
        .into(),
    )
}

/// A warning for the one state in which the model cannot speak at all: no
/// voice in effect, and no free-text description to fall back on.
///
/// This is what a cloning model looks like before any clip has been recorded.
/// Saying it on the card turns a failure at speak time into a state the user
/// can see and fix.
///
/// A model that takes described voices is exempt: it has nothing to enumerate
/// and needs nothing chosen, so an empty list there is not a problem.
pub(super) fn voice_warning<'a>(
    backend: &'a BackendInfo,
    model: &str,
    app: &'a AppModel,
) -> Option<Element<'a, Message>> {
    if !model_voice::shows(&app.voice, &backend.source, model) {
        return None;
    }
    let detail = voice_warning_text(model_voice::warning(&app.voice)?);
    // The same warning glyph the VRAM notice uses, so the two read as one kind
    // of message rather than two.
    Some(
        row![
            icons::phosphor_warning(icons::WARNING, 16.0),
            text::body(detail),
        ]
        .spacing(cosmic::theme::spacing().space_xxs)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// The wording [`voice_warning`] shows: which way out of "nothing to speak in"
/// this user actually has.
fn voice_warning_text(warning: VoiceWarning) -> String {
    match warning {
        // Something to pick, and the dropdown offering it is directly above.
        VoiceWarning::PickACloned => crate::fl!("no-built-in-voices-pick-a-cloned-voice"),
        VoiceWarning::RecordOne => crate::fl!("no-built-in-voices-record-one"),
        // Nothing to send the user to, so the line states the fact and stops.
        VoiceWarning::NoneChosen => crate::fl!("no-cloned-voice-chosen"),
    }
}

#[cfg(test)]
mod tests {
    //! Pin which way out the "nothing to speak in" line offers. The dropdown
    //! and the Voices tab are different answers, and offering the wrong one
    //! is the whole failure mode: a user with a library told to go and record.
    use super::voice_warning_text;
    use super_voice_app_core::pages::model_voice::warning_for;

    /// Which verb the line reaches for, lowercased. The wording is meant to be
    /// edited; which way out it points is not, so that is all these assert on.
    fn routes(has_choices: bool, clones: bool) -> (String, bool, bool) {
        let text = voice_warning_text(warning_for(has_choices, clones)).to_lowercase();
        let picks = text.contains("pick");
        let records = text.contains("record");
        (text, picks, records)
    }

    #[test]
    fn a_library_sends_the_user_to_the_dropdown_above() {
        for clones in [true, false] {
            let (text, picks, records) = routes(true, clones);
            assert!(picks, "{text}");
            assert!(!records, "{text}");
        }
    }

    #[test]
    fn an_empty_library_sends_a_cloning_model_to_the_voices_tab() {
        let (text, _, records) = routes(false, true);
        assert!(records, "{text}");
        assert!(text.contains("voices tab"), "{text}");
    }

    /// Preset voices declared and none shipped: there is nowhere to send
    /// anyone, so the line says what is true and offers nothing.
    #[test]
    fn nothing_to_pick_and_nothing_to_record_just_states_it() {
        let (text, picks, records) = routes(false, false);
        assert!(!picks, "{text}");
        assert!(!records, "{text}");
    }
}
