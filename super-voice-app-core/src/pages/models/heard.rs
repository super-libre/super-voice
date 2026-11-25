// SPDX-License-Identifier: GPL-3.0-only
//! What the daemon's events mean for the cards.
//!
//! Each event that says a stage changed reads that stage again; the read is
//! what the card shows. The events only add what a read can't: that a load
//! started or ended, how far a download is, and a device move that failed.

use serde_json::Value;
use super_voice_shared::models::protocol::{
    DaemonStatusEvent, DownloadProgress, NotificationEvent,
};
use super_voice_shared::tts::models::protocol::DaemonStatusEvent as SynthesisEvent;

use super::{Input, STAGES};
use crate::state::Stage;

/// What `event` means for the cards: nothing, or the inputs to take in.
#[must_use]
pub fn heard(event: &NotificationEvent) -> Vec<Input> {
    let feature = event.data.get("feature").and_then(Value::as_str);
    match event.event_type.as_str() {
        "download_progress" => {
            let Ok(progress) = serde_json::from_value::<DownloadProgress>(event.data.clone())
            else {
                return Vec::new();
            };
            Stage::from_wire(feature, progress.stage)
                .map(|stage| Input::Progress(stage, progress))
                .into_iter()
                .collect()
        }
        "daemon_status_changed" if feature == Some("tts") => {
            serde_json::from_value::<SynthesisEvent>(event.data.clone())
                .map_or_else(|_| Vec::new(), synthesis)
        }
        "daemon_status_changed" => serde_json::from_value::<DaemonStatusEvent>(event.data.clone())
            .map_or_else(|_| Vec::new(), speech_to_text),
        _ => Vec::new(),
    }
}

/// Speech to text's events, each naming its stage by number.
fn speech_to_text(event: DaemonStatusEvent) -> Vec<Input> {
    let at = |number: u32| Stage::speech_to_text(number);
    let input = match event {
        DaemonStatusEvent::Ready {
            model_loaded,
            stage,
            ..
        } => at(stage).map(|stage| Input::Ready {
            stage,
            loaded: model_loaded,
        }),
        // The `ready` a load ends with comes too; this one names the model.
        DaemonStatusEvent::ModelSwitched { stage, .. } => at(stage).map(|stage| Input::Ready {
            stage,
            loaded: true,
        }),
        DaemonStatusEvent::LoadingModel { new_model, stage } => {
            at(stage).map(|stage| Input::LoadStarted {
                stage,
                model: new_model,
            })
        }
        DaemonStatusEvent::LoadingModelForDevice { model, stage, .. }
        | DaemonStatusEvent::SwitchingDevice { model, stage, .. } => {
            at(stage).map(|stage| Input::LoadStarted { stage, model })
        }
        DaemonStatusEvent::DeviceSwitchError { error, stage, .. } => {
            at(stage).map(|stage| Input::DeviceSwitchFailed(stage, error))
        }
        // Transcription's backend, chosen here or by another client.
        DaemonStatusEvent::ActiveBackendChanged { .. } => {
            Some(Input::Changed(Stage::Transcription))
        }
        DaemonStatusEvent::SettingsChanged { setting } => return setting_changed(&setting),
        DaemonStatusEvent::UpdateAvailable { .. } => None,
    };
    input.into_iter().collect()
}

/// Text to speech's events, on its one stage.
fn synthesis(event: SynthesisEvent) -> Vec<Input> {
    let stage = Stage::Synthesis;
    let input = match event {
        SynthesisEvent::Ready { model_loaded, .. } => Input::Ready {
            stage,
            loaded: model_loaded,
        },
        SynthesisEvent::ModelSwitched { .. } => Input::Ready {
            stage,
            loaded: true,
        },
        SynthesisEvent::LoadingModel { new_model } => Input::LoadStarted {
            stage,
            model: new_model,
        },
        SynthesisEvent::LoadingModelForDevice { model, .. }
        | SynthesisEvent::SwitchingDevice { model, .. } => Input::LoadStarted { stage, model },
        SynthesisEvent::DeviceSwitchError { error, .. } => Input::DeviceSwitchFailed(stage, error),
        // A backend selected or deselected elsewhere, with no load after it.
        SynthesisEvent::ActiveBackendChanged { .. } => Input::Changed(stage),
        SynthesisEvent::SettingsChanged { setting } => return setting_changed(&setting),
        _ => return Vec::new(),
    };
    vec![input]
}

/// A shared setting changed: the stages it shows on are read again.
fn setting_changed(setting: &str) -> Vec<Input> {
    match setting {
        "post_processor" => vec![Input::Changed(Stage::PostProcessing)],
        // Whether a model out of memory reads as idle follows the setting.
        "idle_unload" => STAGES.map(Input::Changed).to_vec(),
        // A secret or option stored or cleared is the backend list's to read
        // again, which `Input::ReloadBackends` follows.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(kind: &str, data: Value) -> NotificationEvent {
        NotificationEvent {
            event_type_field: kind.into(),
            event_type: kind.into(),
            client_id: "daemon".into(),
            timestamp: "0".into(),
            data,
        }
    }

    fn status(data: Value) -> Vec<Input> {
        heard(&event("daemon_status_changed", data))
    }

    /// A post-processor's `ready` is its own, never transcription's: reading
    /// it as stage 1's would clear the wrong card.
    #[test]
    fn each_stage_hears_its_own_ready() {
        let inputs = status(json!({"status": "ready", "model_loaded": true, "stage": 2}));
        assert!(matches!(
            inputs[..],
            [Input::Ready {
                stage: Stage::PostProcessing,
                loaded: true
            }]
        ));
        let inputs =
            status(json!({"status": "ready", "model_loaded": false, "model_state": "idle"}));
        assert!(matches!(
            inputs[..],
            [Input::Ready {
                stage: Stage::Transcription,
                loaded: false
            }]
        ));
    }

    /// Text to speech names itself; its stage is synthesis.
    #[test]
    fn a_text_to_speech_load_is_synthesiss() {
        let inputs =
            status(json!({"status": "loading_model", "feature": "tts", "new_model": "kokoro"}));
        assert!(matches!(
            &inputs[..],
            [Input::LoadStarted { stage: Stage::Synthesis, model }] if model == "kokoro"
        ));
    }

    #[test]
    fn a_failed_device_move_is_told() {
        let inputs = status(json!({
            "status": "device_switch_error",
            "error": "out of memory",
            "failed_device": "cuda",
            "model": "base",
        }));
        assert!(matches!(
            &inputs[..],
            [Input::DeviceSwitchFailed(Stage::Transcription, e)] if e == "out of memory"
        ));
    }

    /// Another client's backend reads transcription again.
    #[test]
    fn another_clients_backend_is_read() {
        let inputs = status(json!({"status": "active_backend_changed", "source": "x"}));
        assert!(matches!(inputs[..], [Input::Changed(Stage::Transcription)]));
    }

    /// Sleep When Idle changes what every stage reads as.
    #[test]
    fn idle_unload_reads_every_stage() {
        let inputs = status(json!({"status": "settings_changed", "setting": "idle_unload"}));
        assert_eq!(inputs.len(), STAGES.len());
        assert!(status(json!({"status": "settings_changed", "setting": "volume"})).is_empty());
    }

    /// A download names its stage and feature.
    #[test]
    fn a_download_lands_on_its_stage() {
        let inputs = heard(&event(
            "download_progress",
            json!({
                "model_name": "base",
                "source": "x",
                "stage": 2,
                "current_file": "m.bin",
                "file_index": 0,
                "total_files": 1,
                "bytes_downloaded": 1,
                "total_bytes": 2,
                "percentage": 50.0,
                "status": "downloading",
                "started_at": "",
            }),
        ));
        assert!(matches!(
            inputs[..],
            [Input::Progress(Stage::PostProcessing, _)]
        ));
    }

    /// A stage number the app doesn't know is dropped.
    #[test]
    fn an_unknown_stage_is_dropped() {
        assert!(status(json!({"status": "ready", "model_loaded": true, "stage": 9})).is_empty());
    }
}
