// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::daemon::backends::BackendModel;

const WHISPER: &str = "github.com/x/whisper";

fn view(model: Option<&str>, loaded: bool) -> StageState {
    StageState {
        source: Some(WHISPER.into()),
        name: Some("Whisper".into()),
        enabled: true,
        model: model.map(String::from),
        loaded,
        state: if loaded {
            ModelState::Loaded
        } else {
            ModelState::NotLoaded
        },
        device: None,
    }
}

fn backend(models: &[(&str, &[&str])]) -> BackendInfo {
    BackendInfo {
        source: WHISPER.into(),
        name: "Whisper".into(),
        description: String::new(),
        version: "1.0.0".into(),
        kind: "subprocess".into(),
        allowed_hosts: Vec::new(),
        installed_accel: Vec::new(),
        models: models
            .iter()
            .map(|(name, devices)| BackendModel {
                name: (*name).to_string(),
                provider: String::new(),
                supported_devices: devices.iter().map(ToString::to_string).collect(),
                estimated_vram_bytes: 0,
                multilingual: false,
                supported_languages: Vec::new(),
                primary_language: String::new(),
                realtime: false,
                role: "transcription".into(),
                accepts_context: false,
                clones_voices: false,
            })
            .collect(),
        secrets: Vec::new(),
        options: Vec::new(),
        supports: Vec::new(),
    }
}

/// A page with Whisper filling transcription, `base` staged on the CPU.
fn staged() -> ModelsState {
    let mut state = ModelsState::default();
    state.catalog.record(
        Stage::Transcription,
        vec![backend(&[("base", &["cpu", "gpu"]), ("cloud", &["none"])])],
    );
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), false))),
    );
    update(
        &mut state,
        Input::DevicesLoaded {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            model: "base".into(),
            result: Ok((vec!["cpu".into(), "gpu".into()], Some("cpu".into()))),
        },
    );
    state
}

/// The daemon's remembered model is staged, and what it runs on asked.
#[test]
fn the_daemons_selection_is_staged() {
    let mut state = ModelsState::default();
    let read = update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), false))),
    );
    assert_eq!(
        read.effects,
        [Effect::ReadDevices {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            model: "base".into(),
        }]
    );
    assert_eq!(
        state.picks.model(Stage::Transcription, WHISPER),
        Some("base")
    );
}

/// Load commits the staged device with the model; one change at a time.
#[test]
fn load_commits_the_staged_pick() {
    let mut state = staged();
    assert_eq!(state.picks.device(Stage::Transcription), Some("cpu"));
    assert!(card(&state, Stage::Transcription).can_load);
    let load = update(&mut state, Input::Load(Stage::Transcription));
    assert_eq!(
        load.effects,
        [Effect::Load {
            stage: Stage::Transcription,
            model: "base".into(),
            source: WHISPER.into(),
            device: Some("cpu".into()),
        }]
    );
    assert!(
        update(&mut state, Input::Load(Stage::Transcription))
            .effects
            .is_empty()
    );
    assert!(!card(&state, Stage::Transcription).can_load);
}

/// A failed load shows on the card, and the stage is read again.
#[test]
fn a_failed_load_shows_on_the_card() {
    let mut state = staged();
    update(&mut state, Input::Load(Stage::Transcription));
    let done = update(
        &mut state,
        Input::Done(Stage::Transcription, Change::Load, Err("no memory".into())),
    );
    assert_eq!(done.effects[0], Effect::ReadView(Stage::Transcription));
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Error { message } if message == "no memory"
    ));
    // A failure doesn't hold the card: it can try again.
    assert!(card(&state, Stage::Transcription).can_load);
}

/// An online model needs no device to load.
#[test]
fn an_online_model_loads_without_a_device() {
    let mut state = staged();
    update(
        &mut state,
        Input::PickModel(Stage::Transcription, "cloud".into()),
    );
    update(
        &mut state,
        Input::DevicesLoaded {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            model: "cloud".into(),
            result: Ok((Vec::new(), None)),
        },
    );
    let card = card(&state, Stage::Transcription);
    assert!(card.can_load);
    assert!(!card.no_device);
}

/// A device answer for a model no longer picked is dropped.
#[test]
fn a_late_device_answer_is_dropped() {
    let mut state = staged();
    update(
        &mut state,
        Input::PickModel(Stage::Transcription, "cloud".into()),
    );
    update(
        &mut state,
        Input::DevicesLoaded {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            model: "base".into(),
            result: Ok((vec!["gpu".into()], Some("gpu".into()))),
        },
    );
    assert_eq!(state.picks.device(Stage::Transcription), None);
}

/// A device picked for the running model moves it now; for a staged one, it
/// waits for Load.
#[test]
fn a_running_model_moves_to_the_device_picked() {
    let mut state = staged();
    let staged_only = update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "gpu".into()),
    );
    assert!(staged_only.effects.is_empty());
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), true))),
    );
    let moved = update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "cpu".into()),
    );
    assert_eq!(
        moved.effects,
        [Effect::SetDevice {
            stage: Stage::Transcription,
            model: "base".into(),
            device: "cpu".into(),
            running: true,
        }]
    );
    assert!(state.operations.get(Stage::Transcription).is_pending());
}

/// Download ticks show on the card; a poll that finds nothing ends a
/// download, never a load.
#[test]
fn progress_shows_until_the_download_ends() {
    let mut state = staged();
    let tick = DownloadProgress {
        model_name: "base".into(),
        source: WHISPER.into(),
        stage: 1,
        current_file: "model.bin".into(),
        file_index: 0,
        total_files: 1,
        bytes_downloaded: 10,
        total_bytes: 100,
        percentage: 10.0,
        status: "downloading".into(),
        started_at: String::new(),
        eta_seconds: None,
        error: None,
        load: None,
    };
    update(
        &mut state,
        Input::Progress(Stage::Transcription, tick.clone()),
    );
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Provisioning { .. }
    ));
    update(&mut state, Input::Polled(Stage::Transcription, None));
    assert!(state.operations.is_ready(Stage::Transcription));

    update(&mut state, Input::Load(Stage::Transcription));
    update(&mut state, Input::Polled(Stage::Transcription, None));
    assert!(state.operations.get(Stage::Transcription).is_pending());
}

/// A required secret not stored holds the card: Configure first.
#[test]
fn an_unset_secret_holds_the_card() {
    let mut state = staged();
    let mut needs_key = backend(&[("base", &["cpu"])]);
    needs_key
        .secrets
        .push(super_voice_shared::models::backends::BackendSecret {
            name: "api_key".into(),
            label: Some("API key".into()),
            description: String::new(),
            required: true,
        });
    state.catalog.record(Stage::Transcription, vec![needs_key]);
    let card = card(&state, Stage::Transcription);
    assert_eq!(card.unmet, ["API key"]);
    assert!(!card.can_load);
}

/// Picking the backend already there sends nothing.
#[test]
fn picking_the_same_backend_is_not_a_change() {
    let mut state = staged();
    assert!(
        update(
            &mut state,
            Input::PickBackend(Stage::Transcription, Some(WHISPER.into()))
        )
        .effects
        .is_empty()
    );
    let other = update(
        &mut state,
        Input::PickBackend(Stage::Transcription, Some("github.com/x/other".into())),
    );
    assert_eq!(
        other.effects,
        [Effect::SetBackend(
            Stage::Transcription,
            Some("github.com/x/other".into())
        )]
    );
    assert_eq!(state.picks.get(Stage::Transcription), None);
}
