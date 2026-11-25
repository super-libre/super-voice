// SPDX-License-Identifier: GPL-3.0-only
//! The Library page: the backends installed, and the registry's catalog to
//! install more from. Installing, updating and uninstalling one, the install's
//! progress as the daemon reports it, and the catalog's filters.
//!
//! An install that finishes or fails is told to the app as well
//! ([`Effect::Installed`], [`Effect::InstallFailed`]): a framework build the
//! Models page asked for installs like a backend, and its end is that page's
//! to act on.

use super_voice_shared::daemon::http_client::HttpError;
use super_voice_shared::registry::RegistryListResponse;
use super_voice_shared::registry::events::{InstallPhase, RegistryEvent};

use crate::daemon::registry as client;
use crate::state::registry::{
    FeatureFilter, InstallStatus, InstalledFilters, RefreshOutcome, RegistryState, RoleFilter,
};

/// What the Library page keeps beside the registry's state.
#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    /// The backend whose uninstall waits for the user to confirm.
    pub pending_uninstall: Option<String>,
    /// The Installed tab's filters.
    pub installed_filters: InstalledFilters,
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read the catalog; with the registry's index refreshed first, for
    /// Refresh.
    Refresh {
        index: bool,
    },
    /// The catalog, and why a refresh before it failed, when it did; or why
    /// the catalog couldn't be read.
    CatalogLoaded(Result<(RegistryListResponse, Option<String>), String>),
    Search(String),
    IncludeIncompatible(bool),
    Online(Option<bool>),
    Feature(FeatureFilter),
    Role(RoleFilter),
    InstalledOnline(Option<bool>),
    InstalledFeature(FeatureFilter),
    InstalledRole(RoleFilter),
    /// Install was pressed on a backend from the catalog.
    Install(String),
    /// Update was pressed.
    Update(String),
    /// The daemon took the install on, under this id.
    InstallAccepted {
        source: String,
        install_id: String,
    },
    /// The install didn't start: the daemon's words.
    InstallFailedToStart {
        source: String,
        error: String,
    },
    /// The daemon's install events.
    Event(RegistryEvent),
    /// Uninstall was pressed: ask first.
    AskUninstall(String),
    CancelUninstall,
    /// The user confirmed.
    Uninstall(String),
    /// The daemon uninstalled it, or why not.
    Uninstalled {
        source: String,
        result: Result<(), String>,
    },
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the catalog: [`run_catalog`].
    FetchCatalog {
        refresh_index: bool,
    },
    Install(String),
    Update(String),
    Uninstall(String),
    /// Read the installed backends again, and which can fill each stage.
    ReloadBackends,
    /// Read the installed backends and the catalog again: they carry the two
    /// halves of what an install changed.
    ReloadCatalogs,
    /// An install finished.
    Installed {
        source: String,
    },
    /// An install failed, in the daemon's words.
    InstallFailed {
        source: String,
        message: String,
    },
}

/// Take in `input`, and say what to do.
pub fn update(
    registry: &mut RegistryState,
    library: &mut LibraryState,
    input: Input,
) -> Vec<Effect> {
    match input {
        Input::Refresh { index } => vec![Effect::FetchCatalog {
            refresh_index: index,
        }],
        Input::CatalogLoaded(result) => {
            match result {
                Ok((list, refresh_error)) => {
                    registry.backends = list.backends;
                    registry.generated_at = Some(list.generated_at);
                    registry.last_refresh = Some(match refresh_error {
                        // A refresh that fails still lists: the daemon answers
                        // from the copy it saved, which may be out of date.
                        Some(e) => {
                            log::warn!("Couldn't refresh the backend catalog: {e}");
                            RefreshOutcome::Stale
                        }
                        None => RefreshOutcome::Ok,
                    });
                }
                Err(e) => registry.last_refresh = Some(RefreshOutcome::Failed(e)),
            }
            Vec::new()
        }
        Input::Search(_)
        | Input::IncludeIncompatible(_)
        | Input::Online(_)
        | Input::Feature(_)
        | Input::Role(_)
        | Input::InstalledOnline(_)
        | Input::InstalledFeature(_)
        | Input::InstalledRole(_) => {
            filter(registry, library, input);
            Vec::new()
        }
        // A prior failure to start goes, so the card reads Installing.
        Input::Install(source) => {
            registry.install_errors.remove(&source);
            vec![Effect::Install(source)]
        }
        Input::Update(source) => {
            registry.install_errors.remove(&source);
            vec![Effect::Update(source)]
        }
        Input::InstallAccepted { source, install_id } => {
            registry.install_errors.remove(&source);
            registry.installs.insert(
                source,
                InstallStatus {
                    install_id,
                    phase: InstallPhase::Downloading,
                    bytes_done: 0,
                    bytes_total: None,
                    error: None,
                    reason: None,
                },
            );
            Vec::new()
        }
        // No background install to follow: the card says Failed, and why,
        // rather than snapping back to Install.
        Input::InstallFailedToStart { source, error } => {
            log::error!("install({source}) failed to start: {error}");
            let message = HttpError::Other(error).user_message(&super_voice_shared::SUPER_VOICE);
            registry.installs.remove(&source);
            registry
                .install_errors
                .insert(source.clone(), message.clone());
            vec![Effect::InstallFailed { source, message }]
        }
        Input::Event(event) => install_event(registry, event),
        Input::AskUninstall(source) => {
            library.pending_uninstall = Some(source);
            Vec::new()
        }
        Input::CancelUninstall => {
            library.pending_uninstall = None;
            Vec::new()
        }
        // A stale failure goes, so the row reads in progress on a retry.
        Input::Uninstall(source) => {
            library.pending_uninstall = None;
            registry.uninstall_errors.remove(&source);
            vec![Effect::Uninstall(source)]
        }
        Input::Uninstalled { result: Ok(()), .. } => vec![Effect::ReloadBackends],
        Input::Uninstalled {
            source,
            result: Err(e),
        } => {
            log::error!("uninstall({source}) failed: {e}");
            registry.uninstall_errors.insert(source, e);
            Vec::new()
        }
    }
}

/// A filter changed. The catalog holds every entry, so a filter never asks
/// the daemon again.
fn filter(registry: &mut RegistryState, library: &mut LibraryState, input: Input) {
    match input {
        Input::Search(search) => {
            registry.filters.search = search;
        }
        Input::IncludeIncompatible(on) => {
            registry.filters.include_incompatible = on;
        }
        Input::Online(online) => {
            registry.filters.online = online;
        }
        Input::Feature(feature) => {
            registry.filters.feature = feature;
        }
        Input::Role(role) => {
            registry.filters.role = role;
        }
        Input::InstalledOnline(online) => {
            library.installed_filters.online = online;
        }
        Input::InstalledFeature(feature) => {
            library.installed_filters.feature = feature;
        }
        Input::InstalledRole(role) => {
            library.installed_filters.role = role;
        }
        _ => {}
    }
}

/// An install's progress, end or failure, as the daemon reports it.
fn install_event(registry: &mut RegistryState, event: RegistryEvent) -> Vec<Effect> {
    match event {
        RegistryEvent::Progress {
            install_id,
            source,
            phase,
            bytes_done,
            bytes_total,
        } => {
            if let Some(status) = registry.installs.get_mut(&source)
                && status.install_id == install_id
            {
                status.phase = phase;
                if let Some(done) = bytes_done {
                    status.bytes_done = done;
                }
                if let Some(total) = bytes_total {
                    status.bytes_total = Some(total);
                }
            }
            Vec::new()
        }
        // Both catalogs: the installed backends are what the Installed tab
        // draws, and the registry's carries `installed_version`, which decides
        // whether Update is offered. Left stale, the Update just run stays on
        // the menu and does nothing.
        RegistryEvent::Completed { source, .. } => {
            registry.installs.remove(&source);
            registry.install_errors.remove(&source);
            vec![Effect::Installed { source }, Effect::ReloadCatalogs]
        }
        RegistryEvent::Failed {
            install_id,
            source,
            phase,
            error,
            reason,
        } => {
            let Some(status) = registry
                .installs
                .get_mut(&source)
                .filter(|s| s.install_id == install_id)
            else {
                return Vec::new();
            };
            status.error = Some(error);
            status.reason = reason;
            status.phase = phase;
            let message = status.failure().unwrap_or_default();
            vec![Effect::InstallFailed { source, message }]
        }
        RegistryEvent::RefreshCompleted { .. } | RegistryEvent::RefreshFailed { .. } => Vec::new(),
    }
}

/// The Browse tab's entries: the catalog's backends not installed yet that
/// pass its filters. A backend that needs a newer Super Voice is listed
/// whether or not incompatible ones are shown: the user can act on it, with
/// the version to update to.
#[must_use]
pub fn catalog_entries<'a>(
    registry: &'a RegistryState,
    installed: &[crate::daemon::backends::BackendInfo],
) -> Vec<&'a super_voice_shared::registry::RegistryBackend> {
    use super_voice_registry_types::index::supports;
    let installed: std::collections::HashSet<&str> =
        installed.iter().map(|b| b.source.as_str()).collect();
    let filters = &registry.filters;
    registry
        .backends
        .iter()
        .filter(|entry| !installed.contains(entry.source.as_str()))
        .filter(|entry| {
            filters.include_incompatible
                || entry.compatibility.compatible
                || entry.compatibility.needs_client_update
        })
        .filter(|entry| {
            filters
                .online
                .is_none_or(|online| supports::has(&entry.supports, supports::ONLINE) == online)
        })
        .filter(|entry| {
            crate::state::registry::admits(
                filters.feature,
                filters.role,
                entry.models.iter().map(|m| m.product.role.as_str()),
            )
        })
        .filter(|entry| filters.search.is_empty() || entry_matches(entry, &filters.search))
        .collect()
}

/// Whether a catalog entry's name, description, source or a model's name
/// holds `needle`, in any case.
#[must_use]
pub fn entry_matches(entry: &super_voice_shared::registry::RegistryBackend, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    let mut hay = format!(
        "{} {} {}",
        entry.name.to_lowercase(),
        entry
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase(),
        entry.source.to_lowercase(),
    );
    for model in &entry.models {
        hay.push(' ');
        hay.push_str(&model.name.to_lowercase());
    }
    hay.contains(&needle)
}

/// Whether an installed backend passes the Installed tab's filters.
#[must_use]
pub fn installed_matches(
    backend: &crate::daemon::backends::BackendInfo,
    filters: &InstalledFilters,
) -> bool {
    if let Some(online) = filters.online
        && crate::rules::backends::backend_is_online(backend) != online
    {
        return false;
    }
    crate::state::registry::admits(
        filters.feature,
        filters.role,
        backend.models.iter().map(|m| m.role.as_str()),
    )
}

/// Read the catalog, every entry included (the filters are the app's), with
/// the registry's index refreshed first when `refresh_index`.
pub async fn run_catalog(refresh_index: bool) -> Input {
    let filters = client::ListFilters {
        include_incompatible: Some(true),
        ..Default::default()
    };
    let refresh_error = if refresh_index {
        client::refresh().await.err().map(|e| e.to_string())
    } else {
        None
    };
    Input::CatalogLoaded(
        client::list(&filters)
            .await
            .map(|list| (list, refresh_error))
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_install(source: String) -> Input {
    match client::install_by_source(&source, None).await {
        Ok(accepted) => Input::InstallAccepted {
            source,
            install_id: accepted.install_id,
        },
        Err(e) => Input::InstallFailedToStart {
            source,
            error: e.to_string(),
        },
    }
}

/// Update `source`. An update with nothing to install corrects the catalog
/// that offered it: it reads as finished.
pub async fn run_update(source: String) -> Input {
    match client::update(&source).await {
        Ok(answer) if answer.noop => {
            log::info!("update({source}) noop — already at {}", answer.to_version);
            Input::Event(RegistryEvent::Completed {
                install_id: String::new(),
                source,
                version: answer.to_version,
            })
        }
        Ok(answer) => Input::InstallAccepted {
            source,
            install_id: answer.install_id.unwrap_or_default(),
        },
        Err(e) => Input::InstallFailedToStart {
            source,
            error: e.to_string(),
        },
    }
}

/// Whether a stage is filled by `source`, so uninstalling it stops it first:
/// the uninstall question says so. A stage that can't be read counts as not.
/// For an app that doesn't keep the stages itself.
pub async fn in_use(source: &str) -> bool {
    use crate::daemon::client::v1::pipeline::stage::get_stage;
    for stage in super::models::STAGES {
        if get_stage(stage)
            .await
            .is_ok_and(|s| s.source.as_deref() == Some(source))
        {
            return true;
        }
    }
    false
}

pub async fn run_uninstall(source: String) -> Input {
    let result = client::uninstall(&source)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string());
    Input::Uninstalled { source, result }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An install the daemon took on reads as downloading; its progress, end
    /// and failure follow its id.
    #[test]
    fn an_install_follows_its_id_to_the_end() {
        let mut registry = RegistryState::default();
        let mut library = LibraryState::default();
        assert_eq!(
            update(
                &mut registry,
                &mut library,
                Input::Install("whisper".into())
            ),
            [Effect::Install("whisper".into())]
        );
        update(
            &mut registry,
            &mut library,
            Input::InstallAccepted {
                source: "whisper".into(),
                install_id: "i1".into(),
            },
        );
        update(
            &mut registry,
            &mut library,
            Input::Event(RegistryEvent::Progress {
                install_id: "other".into(),
                source: "whisper".into(),
                phase: InstallPhase::Verifying,
                bytes_done: Some(9),
                bytes_total: None,
            }),
        );
        assert_eq!(
            registry.installs["whisper"].bytes_done, 0,
            "another install's"
        );
        let done = update(
            &mut registry,
            &mut library,
            Input::Event(RegistryEvent::Completed {
                install_id: "i1".into(),
                source: "whisper".into(),
                version: "0.2.1".into(),
            }),
        );
        assert_eq!(
            done,
            [
                Effect::Installed {
                    source: "whisper".into()
                },
                Effect::ReloadCatalogs
            ]
        );
        assert!(registry.installs.is_empty());
    }

    /// Uninstall asks first; a failure stays on its row.
    #[test]
    fn uninstall_asks_first_and_keeps_its_failure() {
        let mut registry = RegistryState::default();
        let mut library = LibraryState::default();
        update(
            &mut registry,
            &mut library,
            Input::AskUninstall("kokoro".into()),
        );
        assert_eq!(library.pending_uninstall.as_deref(), Some("kokoro"));
        assert_eq!(
            update(
                &mut registry,
                &mut library,
                Input::Uninstall("kokoro".into())
            ),
            [Effect::Uninstall("kokoro".into())]
        );
        assert_eq!(library.pending_uninstall, None);
        update(
            &mut registry,
            &mut library,
            Input::Uninstalled {
                source: "kokoro".into(),
                result: Err("busy".into()),
            },
        );
        assert_eq!(registry.uninstall_errors["kokoro"], "busy");
    }
}
