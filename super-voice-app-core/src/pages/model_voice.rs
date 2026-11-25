// SPDX-License-Identifier: GPL-3.0-only
//! The synthesis model's voice, on its Models card: which voice it speaks in,
//! which it could, and setting or clearing its own.
//!
//! The sibling of [`super::model_language`], and shaped the same way: what is
//! set and what may be set arrive from two endpoints and are held apart,
//! because choosing a voice rewrites the first and cannot change the second.
//!
//! One model is focused at a time ([`VoiceState::target`]), and answers for
//! any other are dropped: a read that lands after the user moved to another
//! model belongs to the model they left.

use crate::daemon::client::v1::pipeline::voice as client;
use crate::state::voice::{VoiceChoice, VoiceResolution, VoiceState};

/// What happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The card shows this model: read its voice. An empty pair shows none.
    Focus { source: String, model: String },
    /// Read the focused model's voice again: the library it draws on, or its
    /// voice, changed.
    Reload,
    /// The daemon said how the model's voice resolves.
    Loaded {
        source: String,
        model: String,
        block: VoiceResolution,
    },
    /// The voices the model can be set to.
    Listed {
        source: String,
        model: String,
        voices: Vec<VoiceChoice>,
    },
    /// The user picked a voice for the focused model, or its own default
    /// (`None`).
    Picked(Option<String>),
    /// Reading or storing it failed: why.
    Failed(String),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// [`run_read`].
    Read { source: String, model: String },
    /// [`run_list`].
    List { source: String, model: String },
    /// [`run_save`]: a voice id, or `None` to clear the model's own.
    Save {
        source: String,
        model: String,
        voice: Option<String>,
    },
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotSaveVoice(String),
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the voice's notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn effect(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            ..Self::default()
        }
    }
}

fn focused(state: &VoiceState, source: &str, model: &str) -> bool {
    state
        .target
        .as_ref()
        .is_some_and(|(s, m)| s == source && m == model)
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut VoiceState, input: Input) -> Outcome {
    match input {
        Input::Focus { source, model } => {
            if !focused(state, &source, &model) {
                state.resolution = None;
                state.choices.clear();
            }
            if source.is_empty() || model.is_empty() {
                state.target = None;
                return Outcome::default();
            }
            state.target = Some((source.clone(), model.clone()));
            Outcome::effect(Effect::Read { source, model })
        }
        Input::Reload => match state.target.clone() {
            Some((source, model)) => Outcome::effect(Effect::Read { source, model }),
            None => Outcome::default(),
        },
        // The list is read once the block is held: the two describe one
        // model, and a list for a model with no block would be read as
        // another's.
        Input::Loaded {
            source,
            model,
            block,
        } => {
            if !focused(state, &source, &model) {
                return Outcome::default();
            }
            state.resolution = Some(block);
            Outcome::effect(Effect::List { source, model })
        }
        Input::Listed {
            source,
            model,
            voices,
        } => {
            if focused(state, &source, &model) {
                state.choices = voices;
            }
            Outcome::default()
        }
        // Addressed to the model the card shows, never to "the active model",
        // so a pick landing after a model switch writes nothing rather than
        // the wrong model's voice.
        Input::Picked(voice) => match state.target.clone() {
            Some((source, model)) => Outcome {
                effects: vec![Effect::Save {
                    source,
                    model,
                    voice,
                }],
                clear_notice: true,
                notice: None,
            },
            None => Outcome::default(),
        },
        Input::Failed(e) => Outcome {
            notice: Some(Notice::CouldNotSaveVoice(e)),
            ..Outcome::default()
        },
    }
}

/// Whether `state` describes `(source, model)`: a card drawing another model
/// must not read it.
#[must_use]
pub fn shows(state: &VoiceState, source: &str, model: &str) -> bool {
    focused(state, source, model)
}

/// The first row of the voice picker: the model's own default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultVoice {
    /// The default voice, by its label.
    Named(String),
    /// The model declares none: nothing is chosen, and it can't speak until
    /// something is. Every cloning model starts here.
    NoneChosen,
}

/// The voice picker for the focused model: its default row, then each voice
/// with whether it is cloned. `None` when there is nothing to pick from: a
/// model whose voices are all described takes free text and has no list, and
/// one that clones has none before a voice is recorded.
#[must_use]
pub fn picker(state: &VoiceState) -> Option<(DefaultVoice, Vec<(&VoiceChoice, bool)>)> {
    let block = state.resolution.as_ref()?;
    if state.choices.is_empty() {
        return None;
    }
    // Named, not just "Default": a user comparing two rows should not have to
    // remember which voice the manifest picked.
    let default = block
        .default
        .as_ref()
        .map_or(DefaultVoice::NoneChosen, |id| {
            DefaultVoice::Named(
                state
                    .choices
                    .iter()
                    .find(|v| &v.id == id)
                    .map_or_else(|| id.clone(), |v| v.label.clone()),
            )
        });
    // Cloned voices are marked. They sit with the presets because they are the
    // same choice to the user, but one is a recording they made, and a list
    // that did not say could put two identical labels side by side.
    let voices = state
        .choices
        .iter()
        .map(|v| (v, v.kind == "cloned"))
        .collect();
    Some((default, voices))
}

/// The voice the model is set to, or `None` while it speaks in its default:
/// the row the picker selects. The stored voice, not the one in effect: with
/// none stored, the default row is the one selected.
#[must_use]
pub fn chosen(state: &VoiceState) -> Option<&str> {
    let id = state.resolution.as_ref()?.model_override.as_deref()?;
    state.choices.iter().any(|v| v.id == id).then_some(id)
}

/// Which way out of "nothing to speak in" the user has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceWarning {
    /// Voices to pick from, in the picker above.
    PickACloned,
    /// None yet, and the model clones: record one on the Voices tab.
    RecordOne,
    /// Neither a list nor a way to fill one: a manifest that declares preset
    /// voices and ships none. Nothing to send the user to.
    NoneChosen,
}

/// The warning for the one state in which the model can't speak at all: no
/// voice in effect, and no free-text description to fall back on. `None` when
/// it can.
#[must_use]
pub fn warning(state: &VoiceState) -> Option<VoiceWarning> {
    let block = state.resolution.as_ref()?;
    if !block.has_no_voice() {
        return None;
    }
    Some(warning_for(
        !state.choices.is_empty(),
        block.kinds.iter().any(|k| k == "cloned"),
    ))
}

/// The warning for a model with `has_choices` voices to pick, which `clones`
/// or not. Never sends someone who has a voice to pick off to record one.
#[must_use]
pub const fn warning_for(has_choices: bool, clones: bool) -> VoiceWarning {
    match (has_choices, clones) {
        (true, _) => VoiceWarning::PickACloned,
        (false, true) => VoiceWarning::RecordOne,
        (false, false) => VoiceWarning::NoneChosen,
    }
}

/// Read how the model's voice resolves.
pub async fn run_read(source: String, model: String) -> Input {
    loaded(source, model.clone(), client::get_model_voice(model).await)
}

/// Read the voices the model can be set to.
pub async fn run_list(source: String, model: String) -> Input {
    match client::list_model_voices(model.clone()).await {
        Ok(voices) => Input::Listed {
            source,
            model,
            voices,
        },
        Err(e) => Input::Failed(e.to_string()),
    }
}

/// Set the model's voice, or clear it back to its default with `None`. The
/// daemon answers with how it now resolves.
pub async fn run_save(source: String, model: String, voice: Option<String>) -> Input {
    let result = match voice {
        Some(voice) => client::set_model_voice(model.clone(), voice).await,
        None => client::clear_model_voice(model.clone()).await,
    };
    loaded(source, model, result)
}

fn loaded(
    source: String,
    model: String,
    result: super_voice_shared::daemon::http_client::HttpResult<VoiceResolution>,
) -> Input {
    match result {
        Ok(block) => Input::Loaded {
            source,
            model,
            block,
        },
        Err(e) => Input::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(source: &str, model: &str) -> Input {
        Input::Focus {
            source: source.into(),
            model: model.into(),
        }
    }

    fn block(effective: Option<&str>, default: Option<&str>, kinds: &[&str]) -> VoiceResolution {
        VoiceResolution {
            effective: effective.map(String::from),
            model_override: None,
            default: default.map(String::from),
            kinds: kinds.iter().map(ToString::to_string).collect(),
        }
    }

    fn choice(id: &str, label: &str, kind: &str) -> VoiceChoice {
        VoiceChoice {
            id: id.into(),
            label: label.into(),
            kind: kind.into(),
        }
    }

    fn loaded(state: &mut VoiceState, model: &str, block: VoiceResolution) -> Outcome {
        update(
            state,
            Input::Loaded {
                source: "src".into(),
                model: model.into(),
                block,
            },
        )
    }

    #[test]
    fn focusing_reads_the_block_and_then_the_list() {
        let mut state = VoiceState::default();
        let outcome = update(&mut state, focus("src", "kokoro"));
        assert_eq!(
            outcome.effects,
            [Effect::Read {
                source: "src".into(),
                model: "kokoro".into()
            }]
        );
        let outcome = loaded(
            &mut state,
            "kokoro",
            block(Some("af"), Some("af"), &["preset"]),
        );
        assert_eq!(
            outcome.effects,
            [Effect::List {
                source: "src".into(),
                model: "kokoro".into()
            }]
        );
    }

    #[test]
    fn answers_for_a_model_no_longer_focused_are_dropped() {
        let mut state = VoiceState::default();
        update(&mut state, focus("src", "old"));
        update(&mut state, focus("src", "new"));
        let outcome = loaded(&mut state, "old", block(Some("af"), None, &[]));
        assert!(outcome.effects.is_empty());
        assert!(state.resolution.is_none());
        update(
            &mut state,
            Input::Listed {
                source: "src".into(),
                model: "old".into(),
                voices: vec![choice("af", "Af", "preset")],
            },
        );
        assert!(state.choices.is_empty());
    }

    /// Reading the same model again keeps its list on screen until the new
    /// one lands.
    #[test]
    fn refocusing_the_same_model_keeps_what_it_shows() {
        let mut state = VoiceState::default();
        update(&mut state, focus("src", "kokoro"));
        loaded(
            &mut state,
            "kokoro",
            block(Some("af"), Some("af"), &["preset"]),
        );
        state.choices = vec![choice("af", "Af", "preset")];
        update(&mut state, focus("src", "kokoro"));
        assert!(state.resolution.is_some());
        assert_eq!(state.choices.len(), 1);
        update(&mut state, focus("src", "other"));
        assert!(state.resolution.is_none());
        assert!(state.choices.is_empty());
    }

    #[test]
    fn a_pick_goes_to_the_focused_model() {
        let mut state = VoiceState::default();
        assert_eq!(
            update(&mut state, Input::Picked(Some("af".into()))),
            Outcome::default()
        );
        update(&mut state, focus("src", "kokoro"));
        let outcome = update(&mut state, Input::Picked(None));
        assert!(outcome.clear_notice);
        assert_eq!(
            outcome.effects,
            [Effect::Save {
                source: "src".into(),
                model: "kokoro".into(),
                voice: None
            }]
        );
        assert_eq!(
            update(&mut state, Input::Failed("down".into())).notice,
            Some(Notice::CouldNotSaveVoice("down".into()))
        );
    }

    #[test]
    fn the_picker_names_the_default_and_marks_cloned_voices() {
        let mut state = VoiceState::default();
        update(&mut state, focus("src", "kokoro"));
        loaded(
            &mut state,
            "kokoro",
            block(Some("af"), Some("af"), &["preset", "cloned"]),
        );
        assert!(picker(&state).is_none());
        state.choices = vec![
            choice("af", "Bella", "preset"),
            choice("me", "Me", "cloned"),
        ];
        let (default, voices) = picker(&state).unwrap();
        assert_eq!(default, DefaultVoice::Named("Bella".into()));
        let marks: Vec<bool> = voices.iter().map(|(_, cloned)| *cloned).collect();
        assert_eq!(marks, [false, true]);
        assert_eq!(chosen(&state), None);

        state.resolution.as_mut().unwrap().model_override = Some("me".into());
        assert_eq!(chosen(&state), Some("me"));
    }

    #[test]
    fn a_cloning_model_without_a_default_has_none_chosen() {
        let mut state = VoiceState::default();
        update(&mut state, focus("src", "clone"));
        loaded(&mut state, "clone", block(None, None, &["cloned"]));
        state.choices = vec![choice("me", "Me", "cloned")];
        assert_eq!(picker(&state).unwrap().0, DefaultVoice::NoneChosen);
        assert_eq!(warning(&state), Some(VoiceWarning::PickACloned));

        state.choices.clear();
        assert_eq!(warning(&state), Some(VoiceWarning::RecordOne));
    }

    /// A model that takes described voices needs nothing chosen.
    #[test]
    fn a_described_voice_needs_no_warning() {
        let mut state = VoiceState::default();
        update(&mut state, focus("src", "describe"));
        loaded(&mut state, "describe", block(None, None, &["described"]));
        assert_eq!(warning(&state), None);
    }

    #[test]
    fn the_warning_never_sends_a_user_with_voices_to_record() {
        assert_eq!(warning_for(true, true), VoiceWarning::PickACloned);
        assert_eq!(warning_for(true, false), VoiceWarning::PickACloned);
        assert_eq!(warning_for(false, true), VoiceWarning::RecordOne);
        assert_eq!(warning_for(false, false), VoiceWarning::NoneChosen);
    }
}
