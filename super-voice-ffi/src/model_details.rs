// SPDX-License-Identifier: GPL-3.0-only
//! A model's language, and the synthesis model's voice, as the Models card
//! shows them, for an app written in another language: a store that runs the
//! core's logic for them (`pages::model_language`, `pages::model_voice`) on
//! the shared runtime, follows the daemon's events, and tells the app whenever
//! they changed.
//!
//! The app focuses the model each stage's card shows. Answers for a model no
//! longer focused are dropped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::events::{self, Event};
use super_voice_app_core::languages::friendly_name;
use super_voice_app_core::pages::model_language::{
    self as language, Button, Focus, ModelKey, Resolved, Row,
};
use super_voice_app_core::pages::model_voice::{self as voice, DefaultVoice, VoiceWarning};
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::language::ModelLanguages;
use super_voice_app_core::state::voice::VoiceState;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;
use crate::behavior::Choice;
use crate::status::PipelineStage;

/// The id of the language picker's Follow global row, and of the voice
/// picker's row for the model's own default: picking it clears the model's
/// own.
pub const OWN_CLEARED: &str = "";

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait ModelDetailsObserver: Send + Sync {
    /// What the cards show changed; draw this.
    fn changed(&self, page: ModelDetailsPage);
}

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ModelDetailsAction {
    /// Picked a language for the model `stage` shows, by its row's id: a tag,
    /// or an empty id for Follow global.
    LanguagePicked { stage: PipelineStage, id: String },
    /// Picked a voice for the synthesis model, by its row's id: a voice, or
    /// an empty id for the model's own default.
    VoicePicked { id: String },
}

/// A model's language button.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelLanguageRow {
    pub stage: PipelineStage,
    pub source: String,
    pub model: String,
    /// The button's label: what the language resolves to. `None` while
    /// nothing is known, and for a model with one language that doesn't say
    /// which: no button then.
    pub label: Option<String>,
    /// Whether the button opens the picker. A model with one language shows
    /// it, with nothing to pick.
    pub pickable: bool,
    /// The picker row to mark: the model's own tag, or empty while it follows
    /// the primary language.
    pub chosen: String,
}

/// The synthesis model's voice picker.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelVoiceRow {
    /// The model it is for; empty when none is focused.
    pub source: String,
    pub model: String,
    /// The voices to pick from: first the model's own default (an empty id),
    /// then each voice, a cloned one marked. Empty when there is nothing to
    /// pick: no picker then.
    pub choices: Vec<Choice>,
    /// The row picked: a voice's id, or empty for the model's default.
    pub chosen: String,
    /// The model can't speak until a voice is chosen: what to do about it.
    pub warning: Option<String>,
}

/// Everything the cards show of a model's language and voice.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelDetailsPage {
    /// One row a focused stage.
    pub languages: Vec<ModelLanguageRow>,
    pub voice: ModelVoiceRow,
    /// A language couldn't be read or set: why, in the user's language.
    pub language_notice: Option<String>,
    /// A voice couldn't be read or set: why.
    pub voice_notice: Option<String>,
}

#[derive(Default)]
struct Held {
    languages: ModelLanguages,
    focus: Focus,
    voice: VoiceState,
    language_notice: Option<String>,
    voice_notice: Option<String>,
}

/// An input for one of the two.
#[derive(Debug)]
enum Step {
    Language(language::Input),
    Voice(voice::Input),
}

/// Work one of the two asked for.
enum Work {
    Language(language::Effect),
    Voice(voice::Effect),
}

/// The store.
#[derive(uniffi::Object)]
pub struct ModelDetailsStore {
    held: Mutex<Held>,
    observer: Arc<dyn ModelDetailsObserver>,
}

#[uniffi::export]
impl ModelDetailsStore {
    /// A store that tells `observer` what changes, and follows changes other
    /// clients make. Nothing is read until the app focuses a model.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ModelDetailsObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// What the cards show now.
    pub fn page(&self) -> ModelDetailsPage {
        snapshot(&self.lock())
    }

    /// `stage`'s card shows `model` of `source`: read its language, and for
    /// synthesis its voice. Empty names show none. Focusing the model already
    /// focused reads it again.
    pub fn focus(self: Arc<Self>, stage: PipelineStage, source: String, model: String) {
        let stage = Stage::from(stage);
        if stage == Stage::Synthesis {
            Arc::clone(&self).dispatch(Step::Voice(voice::Input::Focus {
                source: source.clone(),
                model: model.clone(),
            }));
        }
        self.dispatch(Step::Language(language::Input::Focus(ModelKey::new(
            stage, source, model,
        ))));
    }

    /// The language picker's rows for the model `stage` shows, for `query`:
    /// Follow global (an empty id), Auto-detect when the model offers it, then
    /// its other languages by name. Each other id is its tag.
    pub fn language_rows(&self, stage: PipelineStage, query: &str) -> Vec<Choice> {
        let held = self.lock();
        let Some(key) = held.focus.get(stage.into()) else {
            return Vec::new();
        };
        let offers = held
            .languages
            .offered(key.stage, &key.source, &key.model)
            .unwrap_or_default();
        language::picker_rows(offers, query)
            .into_iter()
            .map(|row| match row {
                Row::FollowGlobal => Choice {
                    id: OWN_CLEARED.to_string(),
                    name: words("follow-global", &[]),
                },
                Row::Language(id, name) => Choice { id, name },
            })
            .collect()
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ModelDetailsAction) {
        let step = match action {
            ModelDetailsAction::LanguagePicked { stage, id } => {
                let Some(key) = self.lock().focus.get(stage.into()).cloned() else {
                    return;
                };
                Step::Language(language::Input::Picked(key, own(id)))
            }
            ModelDetailsAction::VoicePicked { id } => Step::Voice(voice::Input::Picked(own(id))),
        };
        self.dispatch(step);
    }
}

/// A row's id as the model's own value: `None` for the row that clears it.
fn own(id: String) -> Option<String> {
    (id != OWN_CLEARED).then_some(id)
}

impl ModelDetailsStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hand `step` to its logic, tell the app what changed, and start the work
    /// it asked for.
    fn dispatch(self: Arc<Self>, step: Step) {
        let (page, work) = {
            let mut held = self.lock();
            let work: Vec<Work> = match step {
                Step::Language(input) => {
                    let Held {
                        languages, focus, ..
                    } = &mut *held;
                    let outcome = language::update(languages, Some(focus), input);
                    if outcome.clear_notice {
                        held.language_notice = None;
                    }
                    if let Some(language::Notice::CouldNotSaveLanguage(e)) = outcome.notice {
                        held.language_notice =
                            Some(words("language-could-not-save", &[("error", e)]));
                    }
                    outcome.effects.into_iter().map(Work::Language).collect()
                }
                Step::Voice(input) => {
                    let outcome = voice::update(&mut held.voice, input);
                    if outcome.clear_notice {
                        held.voice_notice = None;
                    }
                    if let Some(voice::Notice::CouldNotSaveVoice(e)) = outcome.notice {
                        held.voice_notice = Some(words("voice-could-not-save", &[("error", e)]));
                    }
                    outcome.effects.into_iter().map(Work::Voice).collect()
                }
            };
            (snapshot(&held), work)
        };
        self.observer.changed(page);
        for work in work {
            Arc::clone(&self).run(work);
        }
    }

    /// Start `work`. Its answer comes back through [`Self::dispatch`].
    fn run(self: Arc<Self>, work: Work) {
        RUNTIME.spawn(async move {
            let step = match work {
                Work::Language(effect) => Step::Language(match effect {
                    language::Effect::Read(key) => language::run_read(key).await,
                    language::Effect::ReadOffer(key) => language::run_read_offer(key).await,
                    language::Effect::Save(key, tag) => language::run_save(key, tag).await,
                }),
                Work::Voice(effect) => Step::Voice(match effect {
                    voice::Effect::Read { source, model } => voice::run_read(source, model).await,
                    voice::Effect::List { source, model } => voice::run_list(source, model).await,
                    voice::Effect::Save {
                        source,
                        model,
                        voice,
                    } => voice::run_save(source, model, voice).await,
                }),
            };
            self.dispatch(step);
        });
    }

    /// Read again everything focused: the daemon is back.
    fn reread(self: &Arc<Self>) {
        let keys: Vec<ModelKey> = self.lock().focus.keys().cloned().collect();
        for key in keys {
            Arc::clone(self).dispatch(Step::Language(language::Input::Focus(key)));
        }
        Arc::clone(self).dispatch(Step::Voice(voice::Input::Reload));
    }
}

/// Read again what another client changed, and everything when the daemon is
/// back, until `store` is dropped.
fn follow(store: Weak<ModelDetailsStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            match (&event, events::setting_changed(&event)) {
                (Event::Connected, _) => store.reread(),
                // A model that follows the primary language reads differently.
                (_, Some("language")) => {
                    store.dispatch(Step::Language(language::Input::PrimaryChanged));
                }
                // The model's voice was set, or a voice it can be set to was
                // added, renamed or deleted.
                (_, Some("voice" | "voices")) => store.dispatch(Step::Voice(voice::Input::Reload)),
                _ => {}
            }
        }
    });
}

fn snapshot(held: &Held) -> ModelDetailsPage {
    ModelDetailsPage {
        languages: held
            .focus
            .keys()
            .map(|key| language_row(&held.languages, key))
            .collect(),
        voice: voice_row(&held.voice),
        language_notice: held.language_notice.clone(),
        voice_notice: held.voice_notice.clone(),
    }
}

fn language_row(languages: &ModelLanguages, key: &ModelKey) -> ModelLanguageRow {
    let button = language::button(languages, key);
    ModelLanguageRow {
        stage: pipeline_stage(key.stage),
        source: key.source.clone(),
        model: key.model.clone(),
        pickable: matches!(button, Some(Button::Unread | Button::Pickable(_))),
        label: button.map(|button| button_label(&button)),
        chosen: language::chosen(languages, key).unwrap_or_default(),
    }
}

/// A language button's label, in the settings app's words.
fn button_label(button: &Button) -> String {
    match button {
        Button::Unread => words("language", &[]),
        Button::Fixed(tag) | Button::Pickable(Resolved::Override(tag)) => friendly_name(tag),
        Button::Pickable(Resolved::Global(tag)) => {
            words("language-global", &[("value0", friendly_name(tag))])
        }
        Button::Pickable(Resolved::Default(tag)) => {
            words("language-default", &[("value0", friendly_name(tag))])
        }
    }
}

fn voice_row(state: &VoiceState) -> ModelVoiceRow {
    let (source, model) = state.target.clone().unwrap_or_default();
    let choices = voice::picker(state).map_or_else(Vec::new, |(default, voices)| {
        let default = Choice {
            id: OWN_CLEARED.to_string(),
            name: match default {
                DefaultVoice::NoneChosen => words("no-cloned-voice-chosen", &[]),
                DefaultVoice::Named(label) => words("voice-default", &[("label", label)]),
            },
        };
        std::iter::once(default)
            .chain(voices.into_iter().map(|(v, cloned)| Choice {
                id: v.id.clone(),
                name: if cloned {
                    words("voice-cloned", &[("label", v.label.clone())])
                } else {
                    v.label.clone()
                },
            }))
            .collect()
    });
    ModelVoiceRow {
        source,
        model,
        choices,
        chosen: voice::chosen(state).unwrap_or_default().to_string(),
        warning: voice::warning(state).map(|warning| {
            words(
                match warning {
                    VoiceWarning::PickACloned => "no-built-in-voices-pick-a-cloned-voice",
                    VoiceWarning::RecordOne => "no-built-in-voices-record-one",
                    VoiceWarning::NoneChosen => "no-cloned-voice-chosen",
                },
                &[],
            )
        }),
    }
}

const fn pipeline_stage(stage: Stage) -> PipelineStage {
    match stage {
        Stage::Transcription => PipelineStage::Transcription,
        Stage::PostProcessing => PipelineStage::PostProcessing,
        Stage::Synthesis => PipelineStage::Synthesis,
    }
}

/// `key` in the user's language, with `args`.
fn words(key: &str, args: &[(&str, String)]) -> String {
    crate::text_with(
        key,
        args.iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect::<HashMap<_, _>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use super_voice_app_core::state::language::LanguageResolution;
    use super_voice_app_core::state::voice::{VoiceChoice, VoiceResolution};

    #[test]
    fn a_language_button_reads_in_words() {
        let mut languages = ModelLanguages::default();
        let key = ModelKey::new(Stage::Transcription, "src", "whisper");
        languages.record(
            key.stage,
            key.source.clone(),
            key.model.clone(),
            LanguageResolution {
                effective: Some("de".into()),
                source: "global".into(),
                primary: "en".into(),
            },
        );
        languages.record_offer(
            key.stage,
            key.source.clone(),
            key.model.clone(),
            vec!["de".into(), "en".into()],
        );
        let row = language_row(&languages, &key);
        assert_eq!(row.label.as_deref(), Some("German · global"));
        assert!(row.pickable);
        assert_eq!(row.chosen, "");
    }

    #[test]
    fn the_voice_picker_starts_with_the_default_and_marks_cloned_voices() {
        let state = VoiceState {
            resolution: Some(VoiceResolution {
                effective: Some("af".into()),
                model_override: Some("me".into()),
                default: Some("af".into()),
                kinds: vec!["preset".into(), "cloned".into()],
            }),
            choices: vec![
                VoiceChoice {
                    id: "af".into(),
                    label: "Bella".into(),
                    kind: "preset".into(),
                },
                VoiceChoice {
                    id: "me".into(),
                    label: "Me".into(),
                    kind: "cloned".into(),
                },
            ],
            target: Some(("src".into(), "kokoro".into())),
        };
        let row = voice_row(&state);
        let names: Vec<&str> = row.choices.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Bella · default", "Bella", "Me · cloned"]);
        assert_eq!(row.choices[0].id, OWN_CLEARED);
        assert_eq!(row.chosen, "me");
        assert_eq!(row.warning, None);
    }

    #[test]
    fn the_cleared_row_means_none() {
        assert_eq!(own(String::new()), None);
        assert_eq!(own("fr".into()), Some("fr".into()));
    }
}
