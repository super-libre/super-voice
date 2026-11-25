// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's pipeline steps: which backend fills a stage, and how its
//! model stands, from the stage as the daemon reports it.

use crate::daemon::client::ModelState;
use crate::daemon::client::v1::pipeline::StageState;
use crate::state::Stage;

/// What a stage's step says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// No backend fills the stage.
    Empty,
    /// A backend fills it.
    Filled {
        /// The backend's name: the daemon's, or its source when the daemon
        /// can't name it, so the step never goes blank.
        backend: String,
        model: StepModel,
    },
}

/// How a filled stage's model stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepModel {
    /// It runs, on `device` when the daemon says which.
    Running {
        name: String,
        device: Option<String>,
    },
    /// It is out of memory until its next use, which loads it.
    Idle { name: String },
    /// Nothing is up: none picked, not loaded, or a stage switched on whose
    /// model failed to come up.
    NotLoaded,
}

/// What `stage`'s step says, from `block`.
///
/// A stage with a switch of its own runs only when it is on: post-processing
/// can be on with its model down, and that is not running.
#[must_use]
pub fn step(stage: Stage, block: &StageState) -> Step {
    let Some(source) = block.source.as_deref() else {
        return Step::Empty;
    };
    let backend = block.name.clone().unwrap_or_else(|| source.to_string());
    let switched_on = stage.block().is_none() || block.enabled;
    let model = match block.model.clone() {
        Some(name) if block.state == ModelState::Idle => StepModel::Idle { name },
        Some(name) if switched_on && block.loaded => StepModel::Running {
            name,
            device: block.running_device().map(ToString::to_string),
        },
        _ => StepModel::NotLoaded,
    };
    Step::Filled { backend, model }
}

#[cfg(test)]
mod tests {
    use super::{Step, StepModel, step};
    use crate::daemon::client::ModelState;
    use crate::daemon::client::v1::pipeline::StageState;
    use crate::state::Stage;

    fn block(source: Option<&str>, model: Option<&str>, enabled: bool, loaded: bool) -> StageState {
        StageState {
            source: source.map(ToString::to_string),
            name: source.map(|_| "Tidy".to_string()),
            enabled,
            model: model.map(ToString::to_string),
            loaded,
            state: if loaded {
                ModelState::Loaded
            } else {
                ModelState::NotLoaded
            },
            device: None,
        }
    }

    #[test]
    fn an_empty_stage_is_empty() {
        assert_eq!(
            step(Stage::Synthesis, &block(None, None, false, false)),
            Step::Empty
        );
    }

    /// A post-processor switched on whose model failed to come up says so,
    /// rather than reading as running.
    #[test]
    fn an_enabled_block_whose_model_is_down_is_not_loaded() {
        assert_eq!(
            step(
                Stage::PostProcessing,
                &block(Some("github.com/x/tidy"), Some("rules-v1"), true, false)
            ),
            Step::Filled {
                backend: "Tidy".into(),
                model: StepModel::NotLoaded,
            }
        );
    }

    #[test]
    fn a_running_block_shows_its_model() {
        assert_eq!(
            step(
                Stage::PostProcessing,
                &block(Some("github.com/x/tidy"), Some("rules-v1"), true, true)
            ),
            Step::Filled {
                backend: "Tidy".into(),
                model: StepModel::Running {
                    name: "rules-v1".into(),
                    device: None,
                },
            }
        );
    }

    /// A post-processor switched off isn't running, whatever is loaded.
    #[test]
    fn a_block_switched_off_is_not_loaded() {
        let Step::Filled { model, .. } = step(
            Stage::PostProcessing,
            &block(Some("github.com/x/tidy"), Some("rules-v1"), false, true),
        ) else {
            panic!("a backend fills it");
        };
        assert_eq!(model, StepModel::NotLoaded);
    }

    #[test]
    fn an_idle_model_is_named_without_a_device() {
        let mut asleep = block(Some("github.com/x/whisper"), Some("base"), true, false);
        asleep.state = ModelState::Idle;
        let Step::Filled { model, .. } = step(Stage::Transcription, &asleep) else {
            panic!("a backend fills it");
        };
        assert_eq!(
            model,
            StepModel::Idle {
                name: "base".into()
            }
        );
    }

    /// The daemon can't always name a backend; its source stands in.
    #[test]
    fn an_unnamed_backend_goes_by_its_source() {
        let mut unnamed = block(Some("github.com/x/whisper"), None, true, false);
        unnamed.name = None;
        let Step::Filled { backend, .. } = step(Stage::Transcription, &unnamed) else {
            panic!("a backend fills it");
        };
        assert_eq!(backend, "github.com/x/whisper");
    }
}
