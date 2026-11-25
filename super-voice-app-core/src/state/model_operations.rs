// SPDX-License-Identifier: GPL-3.0-only

//! The model operation each pipeline stage has in flight.
//!
//! The stages provision independently. The daemon will load a transcription
//! model while a post-processor is still downloading, and reports every load,
//! download and failure under its own `stage`.
//!
//! The app used to keep one operation and a number saying whose it was. One
//! stage's work therefore spoke for both: a post-processor download left the
//! transcription card's Load button disabled, and a stage-2 failure cleared
//! the loaded stage-1 model. Each stage gets its own slot here instead, so a
//! card reads, and writes, only its own.
//!
//! A stage number this app does not know is dropped where it arrives (see
//! [`Stage::speech_to_text`]), so a daemon that grows a third stage leaves an
//! older app rendering the two it understands.

use std::time::{Duration, Instant};

use crate::state::Stage;

/// A stage's model operation: provisioning its files, loading them, or done.
#[derive(Debug, Clone, Default)]
pub enum ModelOperationState {
    /// Nothing in flight for this stage.
    #[default]
    Ready,
    /// Provisioning the model's files: verifying the copies already on disk,
    /// and downloading the ones that are missing or stale. One state for both
    /// phases because the daemon reports them through one byte-tracked
    /// progress shape and the UI treats them alike; `progress.status`
    /// (`"verifying"` / `"downloading"`) says which is running, and is the only
    /// thing the card's wording depends on.
    Provisioning {
        target_model: String,
        progress: Box<super_voice_shared::models::protocol::DownloadProgress>,
    },
    /// Loading the model into memory, after any download finished.
    Loading {
        target_model: String,
        /// What started it, for the app to say.
        cause: LoadCause,
        /// What the backend reports of its load — a first-time setup, the
        /// step it is on, how far through — once it reports anything.
        load: Option<super_voice_shared::models::protocol::LoadProgress>,
    },
    /// The operation failed, with the daemon's reason.
    Error { message: String },
}

/// What started a load, for the card to say while the backend says nothing
/// of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadCause {
    /// The user's Load: the switch is starting.
    Switch,
    /// The running model brought back up where it runs.
    Reload,
    /// The running model moving onto `device`.
    Move { device: String },
    /// A load the daemon reported, whoever asked for it.
    Reported,
}

impl ModelOperationState {
    /// Whether the daemon still owes an outcome for this operation. The stall
    /// watchdog and the progress poll both act only on such a stage: `Ready`
    /// needs no poll, and `Error` must keep its banner rather than be cleared
    /// by one.
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Loading { .. } | Self::Provisioning { .. })
    }
}

/// One stage's operation, plus the clock the stall watchdog reads.
/// What a progress report did to a stage: see
/// [`ModelOperations::apply_progress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progressed {
    /// It moved on: provisioning or loading.
    Moved,
    /// It failed; the stage shows why.
    Failed,
    /// It finished or was cancelled.
    Finished,
}

#[derive(Debug, Clone, Default)]
struct StageOperation {
    state: ModelOperationState,
    /// When this stage last showed progress — the operation starting, or any
    /// `download_progress` tick for it. `None` outside an operation. Per stage
    /// because a busy stage would otherwise keep a stalled one's clock alive.
    last_progress_at: Option<Instant>,
}

/// What every pipeline stage has in flight, addressed by the daemon's own
/// stage numbers.
#[derive(Debug, Clone, Default)]
pub struct ModelOperations {
    transcription: StageOperation,
    post_processor: StageOperation,
    synthesis: StageOperation,
}

impl ModelOperations {
    fn slot(&self, stage: Stage) -> &StageOperation {
        match stage {
            Stage::Transcription => &self.transcription,
            Stage::PostProcessing => &self.post_processor,
            Stage::Synthesis => &self.synthesis,
        }
    }

    fn slot_mut(&mut self, stage: Stage) -> &mut StageOperation {
        match stage {
            Stage::Transcription => &mut self.transcription,
            Stage::PostProcessing => &mut self.post_processor,
            Stage::Synthesis => &mut self.synthesis,
        }
    }

    /// What `stage` has in flight.
    #[must_use]
    pub fn get(&self, stage: Stage) -> &ModelOperationState {
        &self.slot(stage).state
    }

    /// Whether `stage` is free to start an operation.
    #[must_use]
    pub fn is_ready(&self, stage: Stage) -> bool {
        matches!(self.get(stage), ModelOperationState::Ready)
    }

    /// The stages still owed an outcome, in pipeline order. What the progress
    /// poll asks about: each stage is polled for its own download, so a
    /// stage-1 answer can never clear a stage-2 card.
    #[must_use]
    pub fn pending_stages(&self) -> Vec<Stage> {
        Stage::ALL
            .into_iter()
            .filter(|&stage| self.get(stage).is_pending())
            .collect()
    }

    /// Record `state` for `stage`, leaving the stall clock alone. For the
    /// terminal writes — an outcome arrived, so there is no longer anything to
    /// time.
    pub fn set(&mut self, stage: Stage, operation: ModelOperationState) {
        self.slot_mut(stage).state = operation;
    }

    /// Finish `stage`'s operation, whatever it was.
    pub fn set_ready(&mut self, stage: Stage) {
        self.set(stage, ModelOperationState::Ready);
    }

    /// Begin (or advance) an operation on `stage`, restarting its stall clock.
    /// For the writes that mean the daemon is working: a switch starting, a
    /// download tick, a load beginning.
    pub fn start(&mut self, stage: Stage, operation: ModelOperationState) {
        let slot = self.slot_mut(stage);
        slot.state = operation;
        slot.last_progress_at = Some(Instant::now());
    }

    /// Put every stage back to idle. For a daemon reconnect, where any
    /// operation the app was tracking belongs to a session that is gone.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Fail every stage whose in-flight operation has shown no progress for
    /// `timeout`, and name them. A stage that has already failed, or has
    /// nothing in flight, is left alone.
    /// Take in a report of how `stage`'s model is coming along: a download
    /// tick, the daemon's poll of one, or the load that follows it.
    ///
    /// - `loading_model`: loading, with what the backend says of its load.
    /// - `error`: failed, with the daemon's reason; empty when the report
    ///   gives none, for the app to word.
    /// - `completed`, `cancelled`: over. Nothing changes here: the answer to
    ///   the request that started it, or the next read of the stage, ends it.
    /// - Anything else (`verifying`, `downloading`, and what the daemon adds
    ///   later): its files are being provisioned. Each tick restarts the stall
    ///   clock.
    pub fn apply_progress(
        &mut self,
        stage: Stage,
        progress: &super_voice_shared::models::protocol::DownloadProgress,
    ) -> Progressed {
        let target_model = progress.model_name.clone();
        match progress.status.as_str() {
            "loading_model" => {
                self.start(
                    stage,
                    ModelOperationState::Loading {
                        target_model,
                        cause: LoadCause::Reported,
                        load: progress.load.clone(),
                    },
                );
                Progressed::Moved
            }
            "error" => {
                let message = progress.error.clone().unwrap_or_default();
                self.set(stage, ModelOperationState::Error { message });
                Progressed::Failed
            }
            "completed" | "cancelled" => Progressed::Finished,
            _ => {
                self.start(
                    stage,
                    ModelOperationState::Provisioning {
                        target_model,
                        progress: Box::new(progress.clone()),
                    },
                );
                Progressed::Moved
            }
        }
    }

    pub fn fail_stalled(&mut self, timeout: Duration, message: &str) -> Vec<Stage> {
        let mut stalled = Vec::new();
        for stage in Stage::ALL {
            let slot = self.slot_mut(stage);
            if !slot.state.is_pending() {
                continue;
            }
            if slot
                .last_progress_at
                .is_some_and(|at| at.elapsed() > timeout)
            {
                slot.state = ModelOperationState::Error {
                    message: message.to_string(),
                };
                slot.last_progress_at = None;
                stalled.push(stage);
            }
        }
        stalled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loading(model: &str) -> ModelOperationState {
        ModelOperationState::Loading {
            target_model: model.to_string(),
            cause: LoadCause::Switch,
            load: None,
        }
    }

    #[test]
    fn a_stage_starts_idle_and_reads_back_what_it_was_given() {
        let mut ops = ModelOperations::default();
        assert!(ops.is_ready(Stage::Transcription));
        assert!(ops.is_ready(Stage::PostProcessing));

        ops.start(Stage::PostProcessing, loading("s1-mini"));
        assert!(!ops.is_ready(Stage::PostProcessing));
        assert!(matches!(
            ops.get(Stage::PostProcessing),
            ModelOperationState::Loading { target_model, .. } if target_model == "s1-mini"
        ));
    }

    /// The reported bug: a post-processor download left the transcription
    /// card's Load button disabled, because one state spoke for both stages.
    #[test]
    fn one_stages_operation_leaves_the_other_free() {
        let mut ops = ModelOperations::default();
        ops.start(Stage::PostProcessing, loading("s1-mini"));
        assert!(
            ops.is_ready(Stage::Transcription),
            "stage 2's load must not block stage 1"
        );

        ops.start(Stage::Transcription, loading("whisper-large"));
        assert!(!ops.is_ready(Stage::Transcription));
        assert!(
            !ops.is_ready(Stage::PostProcessing),
            "and stage 2 keeps its own"
        );

        ops.set_ready(Stage::PostProcessing);
        assert!(ops.is_ready(Stage::PostProcessing));
        assert!(
            !ops.is_ready(Stage::Transcription),
            "finishing one leaves the other"
        );
    }

    #[test]
    fn only_the_stages_still_working_are_polled() {
        let mut ops = ModelOperations::default();
        assert!(ops.pending_stages().is_empty());

        ops.start(Stage::PostProcessing, loading("s1-mini"));
        assert_eq!(ops.pending_stages(), vec![Stage::PostProcessing]);

        ops.start(Stage::Transcription, loading("whisper-large"));
        assert_eq!(
            ops.pending_stages(),
            vec![Stage::Transcription, Stage::PostProcessing]
        );

        // A failure is an outcome: the stage is no longer owed one.
        ops.set(
            Stage::Transcription,
            ModelOperationState::Error {
                message: "no".to_string(),
            },
        );
        assert_eq!(ops.pending_stages(), vec![Stage::PostProcessing]);
    }

    /// A zero timeout makes every in-flight stage overdue at once, which is
    /// the mechanism under test — not the duration.
    #[test]
    fn a_stalled_stage_fails_alone() {
        let mut ops = ModelOperations::default();
        ops.start(Stage::PostProcessing, loading("s1-mini"));
        ops.set(Stage::Transcription, ModelOperationState::Ready);

        let stalled = ops.fail_stalled(Duration::ZERO, "stalled");
        assert_eq!(stalled, vec![Stage::PostProcessing]);
        assert!(matches!(
            ops.get(Stage::PostProcessing),
            ModelOperationState::Error { message } if message == "stalled"
        ));
        assert!(
            ops.is_ready(Stage::Transcription),
            "an idle stage is not failed"
        );

        // Already failed: nothing left to time out, so it is not re-reported.
        assert!(ops.fail_stalled(Duration::ZERO, "stalled").is_empty());
    }

    #[test]
    fn a_reconnect_drops_every_stages_operation() {
        let mut ops = ModelOperations::default();
        ops.start(Stage::Transcription, loading("whisper-large"));
        ops.start(Stage::PostProcessing, loading("s1-mini"));

        ops.reset();
        assert!(ops.is_ready(Stage::Transcription));
        assert!(ops.is_ready(Stage::PostProcessing));
    }
}
