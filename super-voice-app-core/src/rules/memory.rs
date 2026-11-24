// SPDX-License-Identifier: GPL-3.0-only
//! Whether a model looks too big for the GPU it is staged on.

use super_voice_shared::models::protocol::GpuInfo;

use crate::daemon::backends::BackendInfo;
use crate::state::Stage;
use crate::state::model_operations::ModelOperationState;
use crate::state::staged_picks::StagedPicks;

/// Pure VRAM-fit check for a staged load: given the staged `device`, the
/// model's conservative `estimated_vram_bytes`, and the primary GPU's
/// available bytes, returns `(needed, available)` when a **GPU** load looks
/// too big to fit. `None` when the framework is not a GPU one, the model
/// declares no estimate (online / unknown), no GPU memory is known, or it
/// should fit.
#[must_use]
pub fn vram_shortfall(
    device: Option<&str>,
    estimated_vram_bytes: u64,
    gpu_available_bytes: Option<u64>,
) -> Option<(u64, u64)> {
    if matches!(device, None | Some("cpu" | "none")) || estimated_vram_bytes == 0 {
        return None;
    }
    let available = gpu_available_bytes?;
    (estimated_vram_bytes > available).then_some((estimated_vram_bytes, available))
}

/// [`vram_shortfall`] for transcription's staged model: its VRAM estimate
/// against the primary GPU's free memory, or its total when the daemon did
/// not report free.
///
/// Silent while the stage is loading (`operation`). The backend puts the
/// weights on the GPU before it builds its kernels, so the free memory polled
/// then already leaves out what this load has taken, and a model that fits
/// reads as one that doesn't.
#[must_use]
pub fn staged_vram_shortfall(
    operation: &ModelOperationState,
    picks: &StagedPicks,
    backend: &BackendInfo,
    gpus: &[GpuInfo],
) -> Option<(u64, u64)> {
    if matches!(operation, ModelOperationState::Loading { .. }) {
        return None;
    }
    let model_name = picks.get(Stage::Transcription).map(|p| p.model.as_str())?;
    let model = backend.models.iter().find(|m| m.name == model_name)?;
    let gpu_available = gpus.first().map(|g| g.free_bytes.unwrap_or(g.total_bytes));
    vram_shortfall(
        picks.device(Stage::Transcription),
        model.estimated_vram_bytes,
        gpu_available,
    )
}

#[cfg(test)]
mod vram_shortfall_tests {
    //! Pin the staged-load VRAM warning: it fires only for a GPU framework
    //! whose conservative estimate exceeds the GPU's available memory.
    //! Anything else — CPU, a fitting model, an online/unknown estimate of
    //! `0`, or no GPU info — stays silent.
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    /// A GPU load bigger than available memory warns, echoing both amounts.
    #[test]
    fn gpu_over_budget_warns_with_amounts() {
        assert_eq!(
            vram_shortfall(Some("cuda"), 48 * GIB, Some(24 * GIB)),
            Some((48 * GIB, 24 * GIB)),
        );
    }

    /// A model that fits — including exactly filling memory — is silent; the
    /// check is strictly-greater.
    #[test]
    fn gpu_within_budget_is_silent() {
        assert_eq!(vram_shortfall(Some("cuda"), 8 * GIB, Some(24 * GIB)), None);
        assert_eq!(vram_shortfall(Some("cuda"), 24 * GIB, Some(24 * GIB)), None);
    }

    /// Online / unknown models declare a `0` estimate and never warn.
    #[test]
    fn zero_estimate_is_silent() {
        assert_eq!(vram_shortfall(Some("cuda"), 0, Some(GIB)), None);
    }

    /// The warning is GPU-specific — CPU and an unset device stay silent
    /// even when the estimate exceeds memory.
    #[test]
    fn non_gpu_devices_are_silent() {
        assert_eq!(vram_shortfall(Some("cpu"), 48 * GIB, Some(24 * GIB)), None);
        assert_eq!(vram_shortfall(None, 48 * GIB, Some(24 * GIB)), None);
    }

    /// Without known GPU memory there's nothing to judge fit against.
    #[test]
    fn no_gpu_info_is_silent() {
        assert_eq!(vram_shortfall(Some("cuda"), 48 * GIB, None), None);
    }
}
