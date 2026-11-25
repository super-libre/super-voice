// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::daemon::backends::BackendModel;

const WHISPER: &str = "github.com/x/whisper";

fn view(model: Option<&str>, loaded: bool) -> StageState {
    view_in(model, loaded, ModelState::NotLoaded)
}

/// A view whose model is idle when it isn't loaded.
fn idle_view(model: &str) -> StageState {
    view_in(Some(model), false, ModelState::Idle)
}

fn view_in(model: Option<&str>, loaded: bool, unloaded: ModelState) -> StageState {
    StageState {
        source: Some(WHISPER.into()),
        name: Some("Whisper".into()),
        enabled: true,
        model: model.map(String::from),
        loaded,
        state: if loaded { ModelState::Loaded } else { unloaded },
        device: None,
    }
}

fn backend(models: &[(&str, &[&str])]) -> BackendInfo {
    backend_with_builds(models, &[])
}

/// A backend with the builds for `installed` frameworks on disk; none is
/// taken at its word as running everything.
fn backend_with_builds(models: &[(&str, &[&str])], installed: &[&str]) -> BackendInfo {
    BackendInfo {
        source: WHISPER.into(),
        name: "Whisper".into(),
        description: String::new(),
        version: "1.0.0".into(),
        kind: "subprocess".into(),
        allowed_hosts: Vec::new(),
        installed_accel: installed.iter().map(ToString::to_string).collect(),
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
        [
            Effect::ReadDevices {
                stage: Stage::Transcription,
                source: WHISPER.into(),
                model: "base".into(),
            },
            // Its language, before Load.
            Effect::Focus {
                stage: Stage::Transcription,
                source: WHISPER.into(),
                model: "base".into(),
            },
            // What the backend runs on, for its chips.
            Effect::ReadBackendDevices {
                stage: Stage::Transcription,
                source: WHISPER.into(),
            },
        ]
    );
    assert_eq!(
        state.picks.model(Stage::Transcription, WHISPER),
        Some("base")
    );
    // Read again unchanged, nothing is asked twice.
    let again = update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), false))),
    );
    assert_eq!(
        again.effects,
        [Effect::ReadBackendDevices {
            stage: Stage::Transcription,
            source: WHISPER.into(),
        }]
    );
    update(
        &mut state,
        Input::BackendDevicesLoaded {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            result: Ok(vec!["cpu".into()]),
        },
    );
    assert!(
        update(
            &mut state,
            Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), false))),
        )
        .effects
        .is_empty()
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

fn tick(status: &str) -> DownloadProgress {
    DownloadProgress {
        model_name: "base".into(),
        source: WHISPER.into(),
        stage: 1,
        current_file: "model.bin".into(),
        file_index: 0,
        total_files: 1,
        bytes_downloaded: 10,
        total_bytes: 100,
        percentage: 10.0,
        status: status.into(),
        started_at: String::new(),
        eta_seconds: None,
        error: None,
        load: None,
    }
}

/// `staged()` with the backend's CPU build installed and a CUDA one offered.
fn staged_on_cpu_build() -> ModelsState {
    let mut state = staged();
    state.catalog.record(
        Stage::Transcription,
        vec![backend_with_builds(&[("base", &["cpu", "cuda"])], &["cpu"])],
    );
    update(
        &mut state,
        Input::DevicesLoaded {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            model: "base".into(),
            result: Ok((vec!["cpu".into(), "cuda".into()], Some("cpu".into()))),
        },
    );
    state
}

/// A framework whose build isn't installed asks first, and nothing changes
/// until the user answers.
#[test]
fn a_framework_without_its_build_asks_first() {
    let mut state = staged_on_cpu_build();
    let asked = update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "cuda".into()),
    );
    assert_eq!(
        asked.effects,
        [Effect::OfferBuild {
            stage: Stage::Transcription,
            source: WHISPER.into(),
            framework: "cuda".into(),
        }]
    );
    assert!(state.build_question().is_some());
    assert_eq!(state.picks.device(Stage::Transcription), Some("cpu"));

    update(&mut state, Input::CancelBuild);
    assert!(state.pending_build.is_none());
    assert_eq!(state.picks.device(Stage::Transcription), Some("cpu"));
}

/// Agreed, the build installs; once in, the model moves onto it.
#[test]
fn an_agreed_build_installs_then_moves_the_model() {
    let mut state = staged_on_cpu_build();
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), true))),
    );
    update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "cuda".into()),
    );
    let install = update(&mut state, Input::ConfirmBuild);
    assert_eq!(
        install.effects,
        [Effect::InstallBuild {
            source: WHISPER.into(),
            framework: "cuda".into(),
        }]
    );
    assert!(state.build_downloading(Stage::Transcription).is_some());
    assert!(card(&state, Stage::Transcription).build.is_some());
    // One build at a time: another pick waits.
    assert!(
        update(
            &mut state,
            Input::PickDevice(Stage::Transcription, "cpu".into())
        )
        .effects
        .is_empty()
    );
    // Another backend's install isn't this one.
    assert!(
        update(
            &mut state,
            Input::BuildInstalled {
                source: "github.com/x/other".into()
            }
        )
        .effects
        .is_empty()
    );

    let moved = update(
        &mut state,
        Input::BuildInstalled {
            source: WHISPER.into(),
        },
    );
    assert_eq!(
        moved.effects,
        [
            Effect::SetDevice {
                stage: Stage::Transcription,
                model: "base".into(),
                device: "cuda".into(),
                running: true,
            },
            Effect::ReadBackends(Stage::Transcription),
        ]
    );
    assert!(state.pending_build.is_none());
}

/// A build that fails to install says so, and leaves the card as it was.
#[test]
fn a_failed_build_is_told() {
    let mut state = staged_on_cpu_build();
    update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "cuda".into()),
    );
    update(&mut state, Input::ConfirmBuild);
    let failed = update(
        &mut state,
        Input::BuildFailed {
            source: WHISPER.into(),
            error: "offline".into(),
        },
    );
    assert_eq!(
        failed.notice,
        Some(Notice::BuildFailed {
            stage: Stage::Transcription,
            framework: "cuda".into(),
            error: "offline".into(),
        })
    );
    assert!(state.pending_build.is_none());
    assert_eq!(state.picks.device(Stage::Transcription), Some("cpu"));
}

/// A build installed with no record runs everything: no question.
#[test]
fn a_build_without_a_record_runs_any_framework() {
    let mut state = staged();
    let picked = update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "gpu".into()),
    );
    assert!(picked.effects.is_empty());
    assert_eq!(state.picks.device(Stage::Transcription), Some("gpu"));
    assert!(state.pending_build.is_none());
}

/// A load the daemon stops reporting on fails after the timeout; one with
/// nothing under way is left alone.
#[test]
fn a_stalled_load_fails_on_a_tick() {
    let mut state = staged();
    assert!(update(&mut state, Input::Tick).notice.is_none());
    update(&mut state, Input::Load(Stage::Transcription));
    // Not yet: the load just started.
    assert!(update(&mut state, Input::Tick).notice.is_none());
    assert!(state.operations.get(Stage::Transcription).is_pending());
    // Five minutes pass with no word.
    state.operations.fail_stalled(std::time::Duration::ZERO, "");
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Error { message } if message.is_empty()
    ));
}

/// A `ready` that says the model came up ends a load; one that says it left
/// memory doesn't end a load the user started meanwhile.
#[test]
fn a_ready_ends_only_a_load() {
    let mut state = staged();
    update(&mut state, Input::Load(Stage::Transcription));
    let unloaded = update(
        &mut state,
        Input::Ready {
            stage: Stage::Transcription,
            loaded: false,
        },
    );
    assert_eq!(unloaded.effects, [Effect::ReadView(Stage::Transcription)]);
    assert!(state.operations.get(Stage::Transcription).is_pending());
    update(
        &mut state,
        Input::Ready {
            stage: Stage::Transcription,
            loaded: true,
        },
    );
    assert!(state.operations.is_ready(Stage::Transcription));
}

/// A load another client started shows on the card; one this app started
/// keeps what it says.
#[test]
fn a_load_from_elsewhere_shows() {
    let mut state = staged();
    update(
        &mut state,
        Input::LoadStarted {
            stage: Stage::Transcription,
            model: "base".into(),
        },
    );
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Loading {
            cause: LoadCause::Reported,
            ..
        }
    ));

    let mut state = staged();
    update(&mut state, Input::Load(Stage::Transcription));
    update(
        &mut state,
        Input::Progress(Stage::Transcription, tick("downloading")),
    );
    update(
        &mut state,
        Input::LoadStarted {
            stage: Stage::Transcription,
            model: "base".into(),
        },
    );
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Provisioning { .. }
    ));
}

/// A device move the daemon couldn't make shows on the card, and the device
/// it kept is staged again.
#[test]
fn a_failed_device_move_shows_on_the_card() {
    let mut state = staged();
    let failed = update(
        &mut state,
        Input::DeviceSwitchFailed(Stage::Transcription, "no memory".into()),
    );
    assert_eq!(
        failed.notice,
        Some(Notice::DeviceSwitchFailed(
            Stage::Transcription,
            "no memory".into()
        ))
    );
    assert!(failed.effects.contains(&Effect::ReadDevices {
        stage: Stage::Transcription,
        source: WHISPER.into(),
        model: "base".into(),
    }));
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Error { .. }
    ));
}

/// An idle model shows as running, marked idle, and its device can still be
/// set for its next load.
#[test]
fn an_idle_model_shows_ready() {
    let mut state = staged();
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(idle_view("base"))),
    );
    assert_eq!(
        card(&state, Stage::Transcription).shown,
        Shown::Running {
            model: "base".into(),
            idle: true
        }
    );
    let set = update(
        &mut state,
        Input::PickDevice(Stage::Transcription, "gpu".into()),
    );
    assert_eq!(
        set.effects,
        [Effect::SetDevice {
            stage: Stage::Transcription,
            model: "base".into(),
            device: "gpu".into(),
            running: false,
        }]
    );
    // Unloaded for good: it is no longer kept for its next use.
    update(
        &mut state,
        Input::Done(Stage::Transcription, Change::Device, Ok(())),
    );
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("base"), false))),
    );
    assert_eq!(
        card(&state, Stage::Transcription).shown,
        Shown::Staging {
            model: Some("base".into())
        }
    );
}

/// The synthesis model coming up or changing refreshes what the Voices tab
/// offers; another stage's doesn't.
#[test]
fn the_synthesis_model_refreshes_the_voices() {
    let mut state = ModelsState::default();
    let first = update(
        &mut state,
        Input::ViewLoaded(Stage::Synthesis, Ok(view(Some("kokoro"), false))),
    );
    assert!(first.effects.contains(&Effect::VoicesChanged));
    let same = update(
        &mut state,
        Input::ViewLoaded(Stage::Synthesis, Ok(view(Some("kokoro"), false))),
    );
    assert!(!same.effects.contains(&Effect::VoicesChanged));
    let up = update(
        &mut state,
        Input::ViewLoaded(Stage::Synthesis, Ok(view(Some("kokoro"), true))),
    );
    assert!(up.effects.contains(&Effect::VoicesChanged));
    let other = update(
        &mut state,
        Input::ViewLoaded(Stage::PostProcessing, Ok(view(Some("tidy"), true))),
    );
    assert!(!other.effects.contains(&Effect::VoicesChanged));
}

/// The model loaded elsewhere is focused, for its language, even when the
/// card already has a pick on that backend.
#[test]
fn a_model_loaded_elsewhere_is_focused() {
    let mut state = staged();
    let switched = update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(view(Some("cloud"), true))),
    );
    assert!(switched.effects.contains(&Effect::Focus {
        stage: Stage::Transcription,
        source: WHISPER.into(),
        model: "cloud".into(),
    }));
}

/// Another client moved the running model: the picker follows it.
#[test]
fn the_running_device_is_adopted() {
    let mut state = staged();
    let mut moved = view(Some("base"), true);
    moved.device = Some(crate::daemon::client::v1::pipeline::model::StageDevice {
        preference: "gpu".into(),
        resolved_accel: Some("cuda".into()),
    });
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(moved.clone())),
    );
    assert_eq!(state.picks.device(Stage::Transcription), Some("gpu"));

    // Not a device the picker doesn't offer.
    moved.device.as_mut().unwrap().preference = "vulkan".into();
    update(
        &mut state,
        Input::ViewLoaded(Stage::Transcription, Ok(moved)),
    );
    assert_eq!(state.picks.device(Stage::Transcription), Some("gpu"));
}

/// The daemon back after it was gone: what was under way is dropped and every
/// stage read again.
#[test]
fn a_reconnect_drops_what_was_under_way() {
    let mut state = staged();
    update(&mut state, Input::Load(Stage::Transcription));
    update(&mut state, Input::Unload(Stage::PostProcessing));
    let again = update(&mut state, Input::Reconnected);
    assert!(state.operations.is_ready(Stage::Transcription));
    assert!(state.busy.is_empty());
    assert!(again.effects.contains(&Effect::ReadView(Stage::Synthesis)));
    assert!(again.effects.contains(&Effect::ReadGpus));
}

/// A backend's devices answer for the backend still there, and no other.
#[test]
fn a_stale_backend_devices_answer_is_dropped() {
    let mut state = staged();
    update(
        &mut state,
        Input::BackendDevicesLoaded {
            stage: Stage::Transcription,
            source: "github.com/x/other".into(),
            result: Ok(vec!["cpu".into()]),
        },
    );
    assert_eq!(
        state
            .offers
            .backend(Stage::Transcription, "github.com/x/other"),
        None
    );
}

/// A download that ends in an error shows on the card, and one with no
/// reason is left for the app to word.
#[test]
fn a_download_error_shows_on_the_card() {
    let mut state = staged();
    update(&mut state, Input::Load(Stage::Transcription));
    update(
        &mut state,
        Input::Progress(Stage::Transcription, tick("error")),
    );
    assert!(matches!(
        state.operations.get(Stage::Transcription),
        ModelOperationState::Error { message } if message.is_empty()
    ));
}
