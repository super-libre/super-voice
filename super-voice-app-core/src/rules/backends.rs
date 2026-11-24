// SPDX-License-Identifier: GPL-3.0-only
//! What a backend can do, as the apps show it: online or local, what it runs
//! on, whether its models hear a dictation context or speak in cloned voices,
//! and whether an update is on offer.

use super_voice_registry_types::index::supports;

/// The version an update should offer, or `None` for no update.
///
/// Whether an update exists is the daemon's answer, read from
/// `update_available`: it is the side that reads the installed manifest off
/// disk and owns the index, so the comparison lives there and no client
/// re-derives it. This only decides whether to *show* that answer.
///
/// Withheld while an install is in flight for this backend: the chip would
/// otherwise stay clickable during its own update, and every further click
/// would reach a daemon that has nothing left to do.
///
/// The flag rides on the registry catalog, not the backends list, so it goes
/// stale unless that catalog is refetched — which is what left an update
/// offered after the update it describes had already happened.
#[must_use]
pub fn update_offer(
    entry: Option<&super_voice_shared::registry::RegistryBackend>,
    in_flight: bool,
) -> Option<String> {
    if in_flight {
        return None;
    }
    let e = entry?;
    e.update_available.then(|| e.version.clone())
}

/// Whether a backend's models are served by an online provider. Online
/// backends transmit audio to a third-party service, flagged in the UI.
#[must_use]
pub fn backend_is_online(backend: &crate::daemon::backends::BackendInfo) -> bool {
    supports::has(&backend.supports, supports::ONLINE)
}

/// Whether this backend's *installed build* can run models on a GPU. Drives
/// the "GPU" capability chip on the backend card. The daemon decides it from
/// the build on disk, so a CUDA-only backend installed on an AMD host, which
/// lands on its CPU build, does not claim one.
#[must_use]
pub fn backend_supports_gpu(backend: &crate::daemon::backends::BackendInfo) -> bool {
    supports::has(&backend.supports, supports::GPU)
}

/// What a stage's backend can run its models on, as the capability chips read
/// it.
#[derive(Debug, PartialEq, Eq)]
pub struct StageCompute {
    pub gpu: bool,
    pub cpu: bool,
}

/// Whether a backend in the catalog takes the user's dictation context, which
/// earns its card the Contexts chip. The index lists it on the entry, from its
/// models. An index or a daemon from before the list says nothing, and reads
/// as no.
#[must_use]
pub fn entry_accepts_context(entry: &super_voice_shared::registry::RegistryBackend) -> bool {
    supports::has(&entry.supports, supports::DICTATION_CONTEXT)
}

/// Whether a stage's backend is handed the user's dictation context for any of
/// the models it offers that stage, which earns it the Contexts chip there. A
/// backend whose transcription model takes a context and whose post-processor
/// does not shows the chip on stage 1 only. The daemon sends a context only
/// with speech to text's requests, so text to speech's stage never shows it.
#[must_use]
pub fn stage_hears_context(
    stage: crate::state::Stage,
    backend: &crate::daemon::backends::BackendInfo,
) -> bool {
    use crate::state::Stage;
    let role = match stage {
        Stage::Transcription => "transcription",
        Stage::PostProcessing => "post_processor",
        Stage::Synthesis => return false,
    };
    backend
        .models
        .iter()
        .any(|m| m.role == role && m.accepts_context)
}

/// Whether a backend in the catalog has a model that speaks in cloned voices,
/// which earns its card the Voice Cloning chip. The index lists it on the
/// entry, from its models. An index or a daemon from before the list says
/// nothing, and reads as no.
#[must_use]
pub fn entry_clones_voices(entry: &super_voice_shared::registry::RegistryBackend) -> bool {
    supports::has(&entry.supports, supports::VOICE_CLONING)
}

/// Whether a stage's backend offers it a model that speaks in cloned voices,
/// which earns it the Voice Cloning chip there. Only text to speech's stage
/// speaks, so a backend serving both features shows it there alone.
///
/// Any model, not only the stage's role: text to speech's own backend list
/// does not say a model's role, and only a synthesis model may clone.
#[must_use]
pub fn stage_clones_voices(
    stage: crate::state::Stage,
    backend: &crate::daemon::backends::BackendInfo,
) -> bool {
    stage == crate::state::Stage::Synthesis && backend.models.iter().any(|m| m.clones_voices)
}

/// What a stage's backend can run its models on.
///
/// `offered` is the daemon's own answer for the stage's selected backend —
/// `/stt/pipeline/{stage}/device/list`, already scoped to the models this stage
/// would run and narrowed to what the install and host can do. Every other
/// card (the Library's, the picker sheet's) is for a backend no stage has
/// selected, which the daemon has no verb for, so those read the catalog.
#[must_use]
pub fn stage_device_support(
    offered: Option<&[String]>,
    backend: &crate::daemon::backends::BackendInfo,
) -> StageCompute {
    match offered {
        Some(devices) => StageCompute {
            gpu: devices.iter().any(|d| d != "cpu"),
            cpu: devices.iter().any(|d| d == "cpu"),
        },
        None => StageCompute {
            gpu: backend_supports_gpu(backend),
            cpu: backend_supports_cpu(backend),
        },
    }
}

/// Whether `model` is the online sentinel (`supported_devices == ["none"]`)
/// — genuinely no local device to pick, ever.
///
/// The daemon's device list is empty for this case — but an empty list means
/// two different things: this one (nothing to pick because there is nothing
/// local to run), or a local model this specific install cannot run on *any*
/// device (e.g. a GPU-only model with only a CPU asset installed). A caller
/// deciding whether to enable a "Load" action must not conflate the two: only
/// this one needs no device at all. Returns `false` for an unknown model.
#[must_use]
pub fn model_is_online(backend: &crate::daemon::backends::BackendInfo, model: &str) -> bool {
    backend
        .models
        .iter()
        .find(|m| m.name == model)
        .is_some_and(|m| m.supported_devices.iter().any(|d| d == "none"))
}

/// Whether any model this backend serves can run on the CPU. Drives the
/// "CPU" capability chip on the backend card.
#[must_use]
pub fn backend_supports_cpu(backend: &crate::daemon::backends::BackendInfo) -> bool {
    supports::has(&backend.supports, supports::CPU)
}

/// Whether the user pointed this backend at an endpoint of their own — a
/// `base_url` option carrying a value. That value is egress the backend's
/// `allowed_hosts` does not describe, so the Cloud chip has to account for it;
/// the address itself stays out of the card.
///
/// Keyed on the value being present, which is the daemon's own rule: it
/// authorizes whatever the override holds, whether or not that happens to equal
/// a `default` some older daemon still reports. Testing against `default` would
/// hide the line for a user who set the endpoint to the same string, on a card
/// whose whole job is disclosing where audio goes.
#[must_use]
pub fn backend_has_user_url(backend: &crate::daemon::backends::BackendInfo) -> bool {
    backend.options.iter().any(|o| {
        o.name == super_voice_registry_types::manifest::BASE_URL_OPTION
            && o.value.as_ref().is_some_and(|v| !v.trim().is_empty())
    })
}

#[cfg(test)]
mod capability_tests {
    //! Pin the device→capability mapping behind the GPU/CPU chips and the
    //! device picker. GPU capability is a property of what the *install* can
    //! do: `installed_accel` is authoritative when present (a non-`cpu` entry
    //! means GPU-capable), falling back to the manifest's `supported_devices`
    //! (`cuda`/`metal`/`gpu` count as GPU, `cpu` as CPU) when there is no
    //! install record. The online sentinel `none` counts as neither. A
    //! backend aggregates capability across every model it serves, so one GPU
    //! model and one CPU model surface both chips.
    use super::*;
    use crate::daemon::backends::{BackendInfo, BackendModel};

    /// Build a backend whose models declare the given device lists.
    fn backend_with_devices(per_model: &[&[&str]]) -> BackendInfo {
        BackendInfo {
            source: "github.com/super-stt/test".to_string(),
            description: String::new(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            kind: "subprocess".to_string(),
            allowed_hosts: Vec::new(),
            installed_accel: Vec::new(),
            models: per_model
                .iter()
                .enumerate()
                .map(|(i, devices)| BackendModel {
                    name: format!("m{i}"),
                    provider: String::new(),
                    supported_devices: devices.iter().map(|s| (*s).to_string()).collect(),
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

    /// A backend with a transcription model that takes a context and a
    /// post-processor that does not: the Contexts chip shows on stage 1, where
    /// the model hears it, and not on stage 2, where it would not.
    #[test]
    fn the_contexts_chip_follows_the_stages_models() {
        use crate::state::Stage;
        let mut b = backend_with_devices(&[&["cpu"], &["cpu"]]);
        b.models[1].role = "post_processor".into();
        assert!(!stage_hears_context(Stage::Transcription, &b));
        assert!(!stage_hears_context(Stage::PostProcessing, &b));

        b.models[0].accepts_context = true;
        assert!(stage_hears_context(Stage::Transcription, &b));
        assert!(!stage_hears_context(Stage::PostProcessing, &b));

        b.models[1].accepts_context = true;
        assert!(stage_hears_context(Stage::PostProcessing, &b));
    }

    /// Text to speech is never handed a context, so its stage shows no chip
    /// even for a backend that serves both features and takes one for speech
    /// to text.
    #[test]
    fn synthesis_shows_no_contexts_chip() {
        let mut b = backend_with_devices(&[&["cpu"], &["cpu"]]);
        b.models[1].role = "synthesis".into();
        b.models[0].accepts_context = true;
        b.supports = vec!["dictation_context".into()];
        assert!(!stage_hears_context(crate::state::Stage::Synthesis, &b));
    }

    /// The Voice Cloning chip shows on text to speech's stage for a backend
    /// with a model that clones, and on no stage of speech to text's, even for
    /// a backend that serves both features.
    #[test]
    fn the_voice_cloning_chip_shows_on_synthesis_only() {
        use crate::state::Stage;
        let mut b = backend_with_devices(&[&["cpu"], &["cpu"]]);
        b.models[1].role = "synthesis".into();
        assert!(!stage_clones_voices(Stage::Synthesis, &b));

        b.models[1].clones_voices = true;
        assert!(stage_clones_voices(Stage::Synthesis, &b));
        assert!(!stage_clones_voices(Stage::Transcription, &b));
        assert!(!stage_clones_voices(Stage::PostProcessing, &b));
    }

    /// Text to speech's own backend list does not say a model's role, so each
    /// model there reads as transcribing: the chip must not hang on the role.
    #[test]
    fn the_voice_cloning_chip_reads_a_list_without_roles() {
        let mut b = backend_with_devices(&[&["cpu"]]);
        b.models[0].clones_voices = true;
        assert!(stage_clones_voices(crate::state::Stage::Synthesis, &b));
    }

    /// The device and cloud chips read the backend's `supports` list, and
    /// nothing else: the daemon works it out from the models and the install
    /// record. An entry this build does not know is skipped.
    #[test]
    fn the_device_chips_read_the_list() {
        let mut b = backend_with_devices(&[&["cpu", "gpu"]]);
        assert!(!backend_supports_gpu(&b));
        assert!(!backend_supports_cpu(&b));
        assert!(!backend_is_online(&b));
        b.supports = vec!["gpu".into(), "online".into(), "teleports".into()];
        assert!(backend_supports_gpu(&b));
        assert!(!backend_supports_cpu(&b));
        assert!(backend_is_online(&b));
    }

    /// A one-model backend whose model declares `supported` and whose install
    /// record declares `installed`.
    fn backend_with_install(supported: &[&str], installed: &[&str]) -> BackendInfo {
        let mut backend = backend_with_devices(&[supported]);
        backend.models[0].name = "m".to_string();
        backend.installed_accel = installed.iter().map(|a| (*a).to_string()).collect();
        backend
    }

    /// The daemon offers an empty list for two different reasons, and a
    /// caller deciding whether to enable a "Load" action must tell them
    /// apart: the online sentinel needs no device at all, while a GPU-only
    /// model on an install that resolved to CPU-only can be loaded nowhere.
    /// Only the model's own `supported_devices` separates them, which is why
    /// this stayed a local read after the lists moved to the daemon.
    #[test]
    fn only_the_online_sentinel_reads_as_online() {
        let online = backend_with_install(&["none"], &[]);
        assert!(model_is_online(&online, "m"));

        let gpu_only_on_a_cpu_install = backend_with_install(&["gpu"], &["cpu"]);
        assert!(!model_is_online(&gpu_only_on_a_cpu_install, "m"));
    }

    /// The daemon's answer wins over the catalog: it is scoped to the stage's
    /// role and narrowed to this host, so a backend whose manifest claims a
    /// GPU shows no GPU chip once the daemon says it cannot offer one.
    #[test]
    fn the_daemons_list_drives_the_chips_when_it_has_answered() {
        let backend = backend_with_install(&["cpu", "gpu"], &["cuda"]);
        assert_eq!(
            stage_device_support(Some(&["cpu".to_string()]), &backend),
            StageCompute {
                gpu: false,
                cpu: true
            }
        );
        assert_eq!(
            stage_device_support(Some(&["vulkan".to_string(), "cpu".to_string()]), &backend),
            StageCompute {
                gpu: true,
                cpu: true
            }
        );
        // An answered-but-empty list is an answer: nothing local to run.
        assert_eq!(
            stage_device_support(Some(&[]), &backend),
            StageCompute {
                gpu: false,
                cpu: false
            }
        );
    }

    /// No answer — a backend no stage has selected, or one whose answer has
    /// not landed — falls back to the catalog rather than blanking the chips.
    #[test]
    fn an_unanswered_backend_falls_back_to_the_catalog() {
        let listing = |supports: &[&str]| {
            let mut backend = backend_with_install(&["cpu", "gpu"], &["cuda"]);
            backend.supports = supports.iter().map(|s| (*s).to_string()).collect();
            backend
        };
        assert_eq!(
            stage_device_support(None, &listing(&["gpu", "cpu"])),
            StageCompute {
                gpu: true,
                cpu: true
            }
        );
        assert_eq!(
            stage_device_support(None, &listing(&["cpu"])),
            StageCompute {
                gpu: false,
                cpu: true
            }
        );
    }

    #[test]
    fn model_is_online_is_false_for_a_local_model_and_an_unknown_one() {
        let backend = backend_with_install(&["cpu", "gpu"], &["cuda"]);
        assert!(!model_is_online(&backend, "m"));
        assert!(!model_is_online(&backend, "absent"));
    }

    /// Build a backend declaring a `base_url` option with the given effective
    /// value — what `GET /backend/list` reports once the user has (or hasn't) set
    /// one.
    fn backend_with_base_url(value: Option<&str>) -> BackendInfo {
        use crate::daemon::backends::BackendOption;
        let mut b = backend_with_devices(&[&["none"]]);
        b.options = vec![BackendOption {
            name: "base_url".to_string(),
            label: None,
            description: String::new(),
            r#type: Some("string".to_string()),
            default: None,
            choices: Vec::new(),
            min: None,
            max: None,
            step: None,
            required: false,
            value: value.map(ToString::to_string),
        }];
        b
    }

    /// The Cloud chip has to account for egress the manifest does not describe.
    /// A `base_url` the user set is exactly that; an unset or blank one is not,
    /// and must not put a phantom line on the card.
    #[test]
    fn user_url_is_flagged_only_once_a_value_is_set() {
        assert!(backend_has_user_url(&backend_with_base_url(Some(
            "https://gw.example.com"
        ))));
        assert!(!backend_has_user_url(&backend_with_base_url(None)));
        assert!(!backend_has_user_url(&backend_with_base_url(Some("  "))));
        // A backend that declares no such option never flags one.
        assert!(!backend_has_user_url(&backend_with_devices(&[&["none"]])));
    }

    /// The daemon authorizes whatever the override holds, so a value equal to a
    /// `default` an older daemon still reports is still egress the manifest did
    /// not declare. The card must say so rather than compare the two.
    #[test]
    fn user_url_is_flagged_even_when_it_equals_a_reported_default() {
        let mut b = backend_with_base_url(Some("https://api.example.com"));
        b.options[0].default = Some("https://api.example.com".to_string());
        assert!(backend_has_user_url(&b));
    }
}

#[cfg(test)]
mod update_offer_tests {
    //! Pin when the daemon's answer is shown. The comparison itself is the
    //! daemon's — these fix what the card does with it, including the failure
    //! this started from: the flag rides on the registry catalog, so a chip can
    //! survive the update it describes and then do nothing when clicked.
    use super::{entry_accepts_context, update_offer};
    use super_voice_shared::registry::RegistryBackend;

    fn entry(installed: Option<&str>, latest: &str) -> RegistryBackend {
        // Mirrors what the daemon computes, so the fixture cannot claim an
        // update the daemon would not report.
        let update_available = installed
            .is_some_and(|i| super_voice_registry_types::version::update_available(i, latest));
        RegistryBackend {
            id: "y".to_string(),
            backend_id: None,
            source: "github.com/x/y".to_string(),
            version: latest.to_string(),
            name: "Y".to_string(),
            description: None,
            license: "Apache-2.0".to_string(),
            kind: "wasm".to_string(),
            contract: "v4".to_string(),
            min_client: None,
            allowed_hosts: Vec::new(),
            online: true,
            supports_gpu: false,
            supports_cpu: false,
            supports: Vec::new(),
            models: Vec::new(),
            secrets: Vec::new(),
            options: Vec::new(),
            compatibility: super_voice_shared::registry::Compatibility {
                compatible: true,
                selected_asset: None,
                reason: None,
                needs_client_update: false,
            },
            builds: Vec::new(),
            installed_version: installed.map(String::from),
            update_available,
            index_stale: None,
        }
    }

    /// A catalog entry gets the Contexts chip from what the index sums up on
    /// it, and not without, which is also how an index from before the field
    /// reads.
    #[test]
    fn a_catalog_entry_takes_a_context_when_the_index_says_so() {
        let mut e = entry(None, "1.0.0");
        assert!(!entry_accepts_context(&e));
        e.supports = vec!["dictation_context".to_string()];
        assert!(entry_accepts_context(&e));
    }

    /// A catalog entry gets the Voice Cloning chip from what the index sums up
    /// on it, and not without.
    #[test]
    fn a_catalog_entry_clones_voices_when_the_index_says_so() {
        let mut e = entry(None, "1.0.0");
        assert!(!super::entry_clones_voices(&e));
        e.supports = vec!["voice_cloning".to_string()];
        assert!(super::entry_clones_voices(&e));
    }

    #[test]
    fn offers_only_a_newer_version() {
        assert_eq!(
            update_offer(Some(&entry(Some("0.1.0"), "0.1.1")), false),
            Some("0.1.1".to_string())
        );
        // Already current — this is the state that was being drawn from a stale
        // catalog and clicked repeatedly.
        assert_eq!(
            update_offer(Some(&entry(Some("0.1.1"), "0.1.1")), false),
            None
        );
        // An index older than what is installed must not prompt a downgrade.
        assert_eq!(
            update_offer(Some(&entry(Some("0.2.0"), "0.1.1")), false),
            None
        );
    }

    #[test]
    fn withholds_while_an_install_is_in_flight() {
        assert_eq!(
            update_offer(Some(&entry(Some("0.1.0"), "0.1.1")), true),
            None
        );
    }

    #[test]
    fn needs_a_catalog_entry_and_an_installed_version() {
        assert_eq!(update_offer(None, false), None);
        assert_eq!(update_offer(Some(&entry(None, "0.1.1")), false), None);
    }
}
