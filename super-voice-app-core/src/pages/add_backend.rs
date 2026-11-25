// SPDX-License-Identifier: GPL-3.0-only
//! The Library's "Install manually" sheet: a backend from outside the
//! catalog, from a git repository's latest release or a folder on this
//! computer.
//!
//! One flow for both. The source is a choice inside it, and resolving it is a
//! step apart from committing to it: Check fills the preview, Install commits
//! what is in it. The preview is where every outcome lands, a failure as much
//! as a backend found: a rejected resolve is keyed by the pasted URL, which
//! matches no Browse card, so it has nowhere else to show (issue #423).
//!
//! The state is the registry's ([`RegistryState::add_source`] and the fields
//! beside it). The install it starts reports like any other, through
//! [`super::library`]: its key is what the user pointed at.

use super_voice_shared::registry::RegistryBackend;

use crate::daemon::registry as client;
use crate::state::registry::{AddSource, PreviewState, RegistryState};

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The source was picked. Switching empties the preview: it described
    /// the other source.
    Source(AddSource),
    /// The repository URL field changed.
    Url(String),
    /// The folder field changed.
    Folder(String),
    /// Choose was pressed: offer the folder picker.
    Choose,
    /// The folder picker answered; `None` when it was cancelled.
    Picked(Option<String>),
    /// Check was pressed, or the field submitted: resolve the source without
    /// installing it.
    Check,
    /// The source resolved to this backend, or why not, in the daemon's
    /// words.
    Previewed(Result<Box<RegistryBackend>, String>),
    /// One frame of the "reading" spinner, for an app that steps it.
    Spin,
    /// Install was pressed: commit to the previewed backend.
    Install,
    /// An install finished. When it was this sheet's, the sheet's work is
    /// done.
    Installed(String),
}

/// What a source points at: the key the daemon reports the install under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Repository(String),
    Folder(String),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Open the platform's folder picker, and answer with [`Input::Picked`].
    PickFolder,
    /// Resolve the source: [`run_preview`].
    Preview(Target),
    /// Install it: [`run_install`], which answers with the Library's input.
    Install(Target),
    /// Close the sheet: what it previewed is installed.
    Close,
}

/// Take in `input`, and say what to do.
pub fn update(registry: &mut RegistryState, input: Input) -> Vec<Effect> {
    match input {
        Input::Source(source) => {
            if registry.add_source != source {
                registry.add_source = source;
                registry.add_preview = PreviewState::Empty;
            }
            Vec::new()
        }
        // Editing a field retires the preview beside it: it described what
        // was there a moment ago, and leaving it up would let Install commit
        // to a backend the field no longer names.
        Input::Url(url) => {
            if registry.custom_repo_input != url {
                registry.add_preview = PreviewState::Empty;
            }
            registry.custom_repo_input = url;
            Vec::new()
        }
        Input::Folder(path) => {
            if registry.add_folder != path {
                registry.add_preview = PreviewState::Empty;
            }
            registry.add_folder = path;
            Vec::new()
        }
        Input::Choose => vec![Effect::PickFolder],
        Input::Picked(None) => Vec::new(),
        // Choosing a folder is the whole gesture, so it resolves at once:
        // there is nothing further to type and no network call to hold back.
        // A URL needs an explicit Check for both reasons.
        Input::Picked(Some(path)) => {
            registry.add_folder.clone_from(&path);
            registry.add_preview = PreviewState::Checking(path.clone());
            vec![Effect::Preview(Target::Folder(path))]
        }
        Input::Check => check(registry),
        Input::Previewed(Ok(backend)) => {
            registry.add_preview = PreviewState::Ready(backend);
            Vec::new()
        }
        Input::Previewed(Err(message)) => {
            registry.add_preview = PreviewState::Failed(message);
            Vec::new()
        }
        // Wrapping rather than saturating: a spinner that stops turning would
        // be a strange way to find out a check ran that long.
        Input::Spin => {
            registry.add_spin = registry.add_spin.wrapping_add(1);
            Vec::new()
        }
        // Keyed by what the user pointed at: the same string the preview
        // resolved, and the key the install's events carry.
        Input::Install => {
            let Some(target) = target(registry) else {
                return Vec::new();
            };
            registry.install_errors.remove(&registry.add_key());
            vec![Effect::Install(target)]
        }
        // The preview described a backend that is now installed: left up, it
        // would offer to install it again. The source survives: someone who
        // just imported a folder is likelier to import another than to
        // switch to a URL.
        Input::Installed(source) => {
            if registry.add_key().is_empty() || registry.add_key() != source {
                return Vec::new();
            }
            registry.clear_add_sheet();
            vec![Effect::Close]
        }
    }
}

/// Check: resolve the source the sheet names. A folder not named yet is
/// chosen first.
fn check(registry: &mut RegistryState) -> Vec<Effect> {
    match target(registry) {
        Some(target) => {
            registry.add_preview = PreviewState::Checking(registry.add_key());
            vec![Effect::Preview(target)]
        }
        None if registry.add_source == AddSource::Folder => vec![Effect::PickFolder],
        None => Vec::new(),
    }
}

/// What the sheet points at, when it names something.
fn target(registry: &RegistryState) -> Option<Target> {
    let key = registry.add_key();
    if key.is_empty() {
        return None;
    }
    Some(match registry.add_source {
        AddSource::Repository => Target::Repository(key),
        AddSource::Folder => Target::Folder(key),
    })
}

/// The backend Install would install: the one previewed, when this computer
/// can run it and no install of it is under way.
#[must_use]
pub fn installable(registry: &RegistryState) -> Option<&RegistryBackend> {
    if registry.installs.contains_key(&registry.add_key()) {
        return None;
    }
    registry
        .add_preview
        .ready()
        .filter(|b| b.compatibility.compatible)
}

/// How a backend runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunsAs {
    /// A WASM component, sandboxed.
    Wasm,
    /// A native subprocess.
    Subprocess,
    /// A kind this build has no name for, as the manifest spells it.
    Other(String),
}

/// What a person agrees to run by installing a backend: the facts its preview
/// lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub runs_as: RunsAs,
    /// The secrets it can't run without, by label. Empty when none: every
    /// local backend needs none, so a row saying "nothing" would be padding.
    pub needs: Vec<String>,
    /// The hosts it reaches. Empty means it runs offline, which is worth
    /// saying: it is what someone installing a microphone reader looks for.
    pub reaches: Vec<String>,
    /// The build this computer would get: its target, and its frameworks.
    pub built_for: Option<String>,
    pub license: String,
}

/// The facts a preview of `backend` lists.
#[must_use]
pub fn facts(backend: &RegistryBackend) -> Facts {
    Facts {
        runs_as: match backend.kind.as_str() {
            "wasm" => RunsAs::Wasm,
            "subprocess" => RunsAs::Subprocess,
            other => RunsAs::Other(other.to_string()),
        },
        needs: backend
            .secrets
            .iter()
            .filter(|s| s.required)
            .map(|s| s.label.clone())
            .collect(),
        reaches: backend.allowed_hosts.clone(),
        built_for: backend
            .compatibility
            .selected_asset
            .as_ref()
            .filter(|asset| !asset.target.is_empty())
            .map(|asset| {
                if asset.accel.is_empty() {
                    asset.target.clone()
                } else {
                    format!("{}, {}", asset.target, asset.accel.join(", "))
                }
            }),
        license: backend.license.clone(),
    }
}

/// Resolve `target`, and describe the backend it would install. A failure is
/// the daemon's sentence, for whoever just pasted a URL; the log keeps the
/// token and status, which is what made issue #423 diagnosable from a log.
pub async fn run_preview(target: Target) -> Input {
    let result = match &target {
        Target::Repository(url) => client::preview_by_repo_url(url).await,
        Target::Folder(path) => client::preview_by_local_path(path).await,
    };
    Input::Previewed(match result {
        Ok(answer) => Ok(Box::new(answer.backend)),
        Err(e) => {
            log::error!("preview failed: {e}");
            Err(e.user_message(&super_voice_shared::SUPER_VOICE))
        }
    })
}

/// Install `target`. The install reports like a catalog one, through the
/// Library's inputs, keyed by what the user pointed at.
pub async fn run_install(target: Target) -> super::library::Input {
    let (key, result) = match target {
        Target::Repository(url) => {
            let result = client::install_by_repo_url(&url).await;
            (url, result)
        }
        Target::Folder(path) => {
            let result = client::install_by_local_path(&path).await;
            (path, result)
        }
    };
    match result {
        Ok(accepted) => super::library::Input::InstallAccepted {
            source: key,
            install_id: accepted.install_id,
        },
        Err(e) => super::library::Input::InstallFailedToStart {
            source: key,
            error: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(compatible: bool) -> Box<RegistryBackend> {
        Box::new(
            serde_json::from_value(serde_json::json!({
                "id": "mine",
                "source": "github.com/me/mine",
                "version": "0.1.0",
                "name": "Mine",
                "license": "MIT",
                "kind": "wasm",
                "contract": "v4",
                "online": false,
                "supports_gpu": false,
                "supports_cpu": true,
                "models": [],
                "secrets": [
                    { "name": "key", "label": "API key", "required": true },
                    { "name": "org", "label": "Organization", "required": false }
                ],
                "options": [],
                "compatibility": {
                    "compatible": compatible,
                    "selected_asset": { "target": "wasm32-wasip2", "accel": ["cpu"] }
                },
                "update_available": false
            }))
            .expect("a listing entry"),
        )
    }

    /// Check resolves the URL typed; editing it retires the preview, so
    /// Install can't commit to a backend the field no longer names.
    #[test]
    fn check_resolves_the_url_and_an_edit_retires_it() {
        let mut registry = RegistryState::default();
        assert!(
            update(&mut registry, Input::Check).is_empty(),
            "nothing typed"
        );
        update(&mut registry, Input::Url(" https://x/y ".into()));
        assert_eq!(
            update(&mut registry, Input::Check),
            [Effect::Preview(Target::Repository("https://x/y".into()))]
        );
        assert!(registry.add_preview.is_checking());
        update(&mut registry, Input::Previewed(Ok(backend(true))));
        assert!(installable(&registry).is_some());
        update(&mut registry, Input::Url("https://x/z".into()));
        assert!(matches!(registry.add_preview, PreviewState::Empty));
        assert!(installable(&registry).is_none());
    }

    /// Check with no folder named offers the picker; a folder picked is
    /// checked at once.
    #[test]
    fn a_folder_is_chosen_then_checked() {
        let mut registry = RegistryState::default();
        update(&mut registry, Input::Source(AddSource::Folder));
        assert_eq!(update(&mut registry, Input::Check), [Effect::PickFolder]);
        assert!(update(&mut registry, Input::Picked(None)).is_empty());
        assert_eq!(
            update(&mut registry, Input::Picked(Some("/dev/mine".into()))),
            [Effect::Preview(Target::Folder("/dev/mine".into()))]
        );
        assert_eq!(registry.add_folder, "/dev/mine");
        update(
            &mut registry,
            Input::Previewed(Err("no backend.toml there".into())),
        );
        assert!(matches!(registry.add_preview, PreviewState::Failed(_)));
    }

    /// Install commits what was previewed, only when this computer can run
    /// it; its end empties and closes the sheet, and another install's
    /// doesn't.
    #[test]
    fn install_commits_the_preview_and_its_end_closes_the_sheet() {
        let mut registry = RegistryState::default();
        update(&mut registry, Input::Url("https://x/y".into()));
        update(&mut registry, Input::Previewed(Ok(backend(false))));
        assert!(installable(&registry).is_none(), "can't run here");
        update(&mut registry, Input::Previewed(Ok(backend(true))));
        registry
            .install_errors
            .insert("https://x/y".into(), "earlier".into());
        assert_eq!(
            update(&mut registry, Input::Install),
            [Effect::Install(Target::Repository("https://x/y".into()))]
        );
        assert!(registry.install_errors.is_empty(), "a retry clears it");
        assert!(update(&mut registry, Input::Installed("whisper".into())).is_empty());
        assert_eq!(
            update(&mut registry, Input::Installed("https://x/y".into())),
            [Effect::Close]
        );
        assert!(registry.add_key().is_empty());
        assert!(matches!(registry.add_preview, PreviewState::Empty));
    }

    /// The facts name the secrets it needs, the build it would get, and how
    /// it runs.
    #[test]
    fn the_facts_read_the_manifest() {
        let facts = facts(&backend(true));
        assert_eq!(facts.runs_as, RunsAs::Wasm);
        assert_eq!(facts.needs, ["API key"]);
        assert!(facts.reaches.is_empty());
        assert_eq!(facts.built_for.as_deref(), Some("wasm32-wasip2, cpu"));
        assert_eq!(facts.license, "MIT");
    }
}
