// SPDX-License-Identifier: GPL-3.0-only
//! How ready each feature is and how full the GPU is: the core's readiness
//! (`pages::readiness`) run for the window, whose sidebar and status bar read
//! it.

use std::time::Duration;

use gpui_kit::{AsyncApp, Context, Task, WeakEntity};
use super_voice_app_core::daemon::client::StageState;
use super_voice_app_core::pages::readiness::{
    self as page, Effect, Feature, GpuMemory, Input, Readiness as FeatureReadiness, ReadinessState,
};

use crate::effects;

/// How often the GPU's memory is read again while the daemon answers.
const GPU_REFRESH: Duration = Duration::from_secs(15);

/// The readiness, as a GPUI model.
pub struct Readiness {
    state: ReadinessState,
    /// The GPU timer, while the daemon answers.
    timer: Option<Task<()>>,
}

impl Readiness {
    /// Nothing known until the daemon answers.
    #[must_use]
    pub fn new() -> Self {
        Self::fixed(ReadinessState::default())
    }

    /// A readiness that stays as `state` and asks the daemon nothing: for a
    /// screenshot.
    #[must_use]
    pub fn fixed(state: ReadinessState) -> Self {
        Self { state, timer: None }
    }

    /// How ready `feature` is.
    #[must_use]
    pub fn of(&self, feature: Feature) -> FeatureReadiness {
        page::readiness(&self.state, feature)
    }

    /// `feature`'s stage as the daemon last reported it: its backend and
    /// model. What the Clips page names a clip's voice for.
    #[must_use]
    pub fn stage(&self, feature: Feature) -> Option<&StageState> {
        self.state.views.get(&feature.stage())
    }

    /// The first GPU's memory, when the daemon reported a GPU.
    #[must_use]
    pub fn gpu(&self) -> Option<GpuMemory> {
        page::gpu(&self.state)
    }

    /// Take in `input`: the core decides, this does what it says.
    pub fn send(&mut self, input: Input, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        for effect in outcome.effects {
            match effect {
                Effect::ReadStage(stage) => {
                    effects::spawn(page::run_read_stage(stage), cx, Self::send).detach();
                }
                Effect::ReadGpus => {
                    effects::spawn(page::run_read_gpus(), cx, Self::send).detach();
                }
                Effect::ReadTyping => {
                    effects::spawn(page::run_read_typing(), cx, Self::send).detach();
                }
            }
        }
        if self.state.connected {
            if self.timer.is_none() {
                self.timer = Some(cx.spawn(refresh_gpus));
            }
        } else {
            self.timer = None;
        }
        cx.notify();
    }
}

/// Read the GPU's memory every [`GPU_REFRESH`] while the daemon answers.
async fn refresh_gpus(this: WeakEntity<Readiness>, cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(GPU_REFRESH).await;
        if this
            .update(cx, |readiness, cx| readiness.send(Input::Tick, cx))
            .is_err()
        {
            return;
        }
    }
}
