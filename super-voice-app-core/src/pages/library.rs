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

use std::collections::HashSet;

use super_voice_shared::registry::RegistryBackend;

use crate::daemon::backends::BackendInfo;
use crate::daemon::registry as client;
use crate::pages::add_backend::{Facts, RunsAs, facts};
use crate::pages::readiness::Feature;
use crate::rules::backends::{
    Capabilities, ModelGroup, ModelKind, backend_capabilities, backend_is_online,
    entry_capabilities, entry_needs_key, model_groups, repository_url, update_offer,
};
use crate::state::registry::{
    FeatureFilter, InstallStatus, InstalledFilters, RefreshOutcome, RegistryState, RoleFilter,
};

/// What the Library page keeps beside the registry's state.
#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    /// The backend whose uninstall waits for the user to confirm.
    pub pending_uninstall: Option<String>,
    /// A stage runs the backend whose uninstall waits, so uninstalling it
    /// stops it first. Read after the question is asked: false until then.
    pub pending_in_use: bool,
    /// The Installed tab's filters.
    pub installed_filters: InstalledFilters,
    /// The backends installed, as the daemon last listed them.
    pub backends: Vec<BackendInfo>,
    /// Why the installed backends couldn't be read, when they couldn't.
    pub backends_error: Option<String>,
    /// The installed backends that need a key that isn't stored yet, by
    /// source.
    pub missing_keys: HashSet<String>,
    /// The backend the inspector shows, by source: installed or from the
    /// catalog.
    pub selected: Option<String>,
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The page was reached or shown: read the installed backends and the
    /// catalog again.
    Reload,
    /// Read the catalog; with the registry's index refreshed first, for
    /// Refresh.
    Refresh {
        index: bool,
    },
    /// The installed backends, or why they couldn't be read.
    BackendsLoaded(Result<Vec<BackendInfo>, String>),
    /// Whether an installed backend still needs a key: one it requires is
    /// not stored.
    KeysRead {
        source: String,
        missing: bool,
    },
    /// A backend was picked in the table, or none.
    Select(Option<String>),
    /// Whether a stage runs the backend whose uninstall waits.
    InUse {
        source: String,
        in_use: bool,
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
    /// Read the installed backends again, and which can fill each stage:
    /// [`run_backends`].
    ReloadBackends,
    /// Read which of an installed backend's required keys are stored:
    /// [`run_keys`], with the names of those it requires.
    ReadKeys {
        source: String,
        required: Vec<String>,
    },
    /// Read whether a stage runs `source`: [`run_in_use`].
    ReadInUse(String),
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
        Input::Reload => vec![
            Effect::ReloadBackends,
            Effect::FetchCatalog {
                refresh_index: false,
            },
        ],
        Input::Refresh { index } => vec![Effect::FetchCatalog {
            refresh_index: index,
        }],
        Input::BackendsLoaded(_)
        | Input::KeysRead { .. }
        | Input::Select(_)
        | Input::InUse { .. }
        | Input::AskUninstall(_)
        | Input::CancelUninstall => installed(library, input),
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
        // A stale failure goes, so the row reads in progress on a retry.
        Input::Uninstall(source) => {
            library.pending_uninstall = None;
            library.pending_in_use = false;
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

/// What the page reads of the installed backends: the list, which keys
/// they lack, which is picked; and the uninstall question.
fn installed(library: &mut LibraryState, input: Input) -> Vec<Effect> {
    match input {
        Input::BackendsLoaded(Ok(backends)) => {
            library.backends_error = None;
            library
                .missing_keys
                .retain(|source| backends.iter().any(|b| &b.source == source));
            let reads = backends
                .iter()
                .filter_map(|b| {
                    let required: Vec<String> = b
                        .secrets
                        .iter()
                        .filter(|s| s.required)
                        .map(|s| s.name.clone())
                        .collect();
                    (!required.is_empty()).then(|| Effect::ReadKeys {
                        source: b.source.clone(),
                        required,
                    })
                })
                .collect();
            library.backends = backends;
            reads
        }
        // A failed read keeps the list as it was: the next event or visit
        // reads it again.
        Input::BackendsLoaded(Err(e)) => {
            log::warn!("Couldn't read the installed backends: {e}");
            library.backends_error = Some(e);
            Vec::new()
        }
        Input::KeysRead { source, missing } => {
            if missing {
                library.missing_keys.insert(source);
            } else {
                library.missing_keys.remove(&source);
            }
            Vec::new()
        }
        Input::Select(source) => {
            library.selected = source;
            Vec::new()
        }
        Input::InUse { source, in_use } => {
            if library.pending_uninstall.as_deref() == Some(source.as_str()) {
                library.pending_in_use = in_use;
            }
            Vec::new()
        }
        // Whether a stage runs it is read after the question is up: not until
        // the answer comes does the question say so.
        Input::AskUninstall(source) => {
            library.pending_uninstall = Some(source.clone());
            library.pending_in_use = false;
            vec![Effect::ReadInUse(source)]
        }
        Input::CancelUninstall => {
            library.pending_uninstall = None;
            library.pending_in_use = false;
            Vec::new()
        }
        _ => Vec::new(),
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

/// Read the installed backends.
pub async fn run_backends() -> Input {
    Input::BackendsLoaded(
        crate::daemon::client::list_backends()
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Read whether `source` still needs a key: one of `required` is not stored.
/// A read that fails says nothing is missing, rather than warn on a guess.
pub async fn run_keys(source: String, required: Vec<String>) -> Input {
    let missing = match crate::daemon::client::list_backend_secrets(source.clone()).await {
        Ok(stored) => required.iter().any(|name| {
            !stored
                .iter()
                .any(|(stored_name, set)| stored_name == name && *set)
        }),
        Err(e) => {
            log::warn!("Couldn't read which of {source}'s keys are stored: {e}");
            false
        }
    };
    Input::KeysRead { source, missing }
}

/// Read whether a stage runs `source`, for the uninstall question.
pub async fn run_in_use(source: String) -> Input {
    let in_use = in_use(&source).await;
    Input::InUse { source, in_use }
}

/// Where a backend runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    /// On this computer.
    Local,
    /// A service online.
    Cloud,
}

/// Where an install of a backend stands: its own, or its update's.
#[derive(Debug, Clone, PartialEq)]
pub enum Installing {
    /// None under way.
    Idle,
    /// Under way: its step, and how far, 0 to 1, once the size is known.
    Running {
        phase: InstallPhase,
        fraction: Option<f32>,
    },
    /// It failed, or didn't start: why.
    Failed(String),
}

/// What the State column says of a row.
#[derive(Debug, Clone, PartialEq)]
pub enum RowState {
    /// From the catalog, ready to install.
    Available,
    /// Installed, nothing to say.
    Installed,
    /// Installed, and this version is ready to update to.
    UpdateReady(String),
    /// Installed, and a key it needs isn't stored.
    NeedsKey,
    /// Installing or updating.
    Installing {
        phase: InstallPhase,
        fraction: Option<f32>,
    },
    /// The install or update failed: why.
    Failed(String),
    /// From the catalog, and it needs a newer Super Voice: this version,
    /// when the catalog says.
    NeedsNewerApp(Option<String>),
    /// From the catalog, and this computer can't run it: why, when the
    /// daemon says.
    CannotRun(Option<String>),
}

/// One framework a backend runs on, or can be installed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framework {
    /// The daemon's id: `cuda`, `vulkan`, `cpu`.
    pub accel: String,
    /// What downloading its build takes, when it isn't on this computer.
    pub download: Option<u64>,
}

/// A row of the Library's table and what its inspector shows: an installed
/// backend, or one from the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub source: String,
    pub name: String,
    /// The version installed, or the catalog's for one not installed.
    pub version: String,
    pub description: Option<String>,
    pub installed: bool,
    /// The features its models serve, in the sidebar's order.
    pub features: Vec<Feature>,
    /// What its models are for, in pipeline order.
    pub kinds: Vec<ModelKind>,
    pub runs: Runs,
    pub capabilities: Capabilities,
    /// Its models, by kind.
    pub models: Vec<ModelGroup>,
    /// What installing it agrees to: how it runs, what it needs and reaches,
    /// its license. The license and the build are the catalog's, so an
    /// installed backend the catalog doesn't list has neither.
    pub facts: Facts,
    /// The frameworks it runs on here (installed), or that this computer can
    /// run a build of (catalog), best first.
    pub frameworks: Vec<Framework>,
    /// Its repository's web page.
    pub repository: String,
    pub state: RowState,
    /// Its install or update.
    pub installing: Installing,
    /// The version an update would bring.
    pub update_to: Option<String>,
    /// Why its last uninstall failed.
    pub uninstall_error: Option<String>,
    /// From the catalog, and a key is required before it runs.
    pub needs_key: bool,
}

impl Row {
    /// Whether Install can be pressed: from the catalog, this computer can
    /// run it, and no install of it is under way.
    #[must_use]
    pub fn installable(&self) -> bool {
        !self.installed
            && matches!(self.state, RowState::Available | RowState::Failed(_))
            && !matches!(self.installing, Installing::Running { .. })
    }
}

/// The table's rows: the installed backends, then the catalog's, each that
/// passes the toolbar's search and filters. One set of filters for both, so
/// the two can be compared.
#[must_use]
pub fn rows(registry: &RegistryState, library: &LibraryState) -> Vec<Row> {
    let filters = &registry.filters;
    let installed_filters = InstalledFilters {
        online: filters.online,
        feature: filters.feature,
        role: filters.role,
    };
    let catalog = registry.by_source();
    let installed = library
        .backends
        .iter()
        .filter(|b| installed_matches(b, &installed_filters))
        .filter(|b| filters.search.is_empty() || backend_matches(b, &filters.search))
        .map(|b| {
            installed_row(
                registry,
                library,
                b,
                catalog.get(b.source.as_str()).copied(),
            )
        });
    let available = catalog_entries(registry, &library.backends)
        .into_iter()
        .map(|entry| catalog_row(registry, entry));
    installed.chain(available).collect()
}

/// The row of `source`, whether the filters show it or not: the inspector
/// keeps showing what was picked.
#[must_use]
pub fn row(registry: &RegistryState, library: &LibraryState, source: &str) -> Option<Row> {
    if let Some(backend) = library.backends.iter().find(|b| b.source == source) {
        let entry = registry.backends.iter().find(|e| e.source == source);
        return Some(installed_row(registry, library, backend, entry));
    }
    registry
        .backends
        .iter()
        .find(|e| e.source == source)
        .map(|entry| catalog_row(registry, entry))
}

/// The row the inspector shows, when one is picked and still exists.
#[must_use]
pub fn selected(registry: &RegistryState, library: &LibraryState) -> Option<Row> {
    row(registry, library, library.selected.as_deref()?)
}

/// How many installed backends have an update ready: the sidebar's mark.
#[must_use]
pub fn updates_ready(registry: &RegistryState, library: &LibraryState) -> usize {
    let catalog = registry.by_source();
    library
        .backends
        .iter()
        .filter(|b| {
            update_offer(
                catalog.get(b.source.as_str()).copied(),
                registry.installs.contains_key(&b.source),
            )
            .is_some()
        })
        .count()
}

/// Whether the toolbar narrows the table: a search, a filter, or
/// incompatible backends shown.
#[must_use]
pub fn filtered(registry: &RegistryState) -> bool {
    let filters = &registry.filters;
    !filters.search.trim().is_empty()
        || filters.online.is_some()
        || filters.feature != FeatureFilter::All
        || filters.role != RoleFilter::All
}

/// How many filters the Filters menu has set, for its button: feature, kind,
/// where it runs, and incompatible backends shown.
#[must_use]
pub fn filters_set(registry: &RegistryState) -> usize {
    let filters = &registry.filters;
    usize::from(filters.feature != FeatureFilter::All)
        + usize::from(filters.feature.has_kinds() && filters.role != RoleFilter::All)
        + usize::from(filters.online.is_some())
        + usize::from(filters.include_incompatible)
}

/// Clear the search and every filter.
pub fn clear_filters(registry: &mut RegistryState) {
    let include_incompatible = registry.filters.include_incompatible;
    registry.filters = crate::state::registry::Filters {
        include_incompatible,
        ..Default::default()
    };
}

/// When the catalog was made, as the app says it: local time, to the minute.
/// What the daemon wrote, when it isn't a time.
#[must_use]
pub fn catalog_time(generated_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(generated_at).map_or_else(
        |_| generated_at.to_string(),
        |time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

/// Whether an installed backend's name, description, source or a model's
/// name holds `needle`, in any case.
#[must_use]
pub fn backend_matches(backend: &BackendInfo, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    [&backend.name, &backend.description, &backend.source]
        .into_iter()
        .chain(backend.models.iter().map(|m| &m.name))
        .any(|text| text.to_lowercase().contains(&needle))
}

fn installed_row(
    registry: &RegistryState,
    library: &LibraryState,
    backend: &BackendInfo,
    entry: Option<&RegistryBackend>,
) -> Row {
    let installing = installing(registry, &backend.source);
    let update_to = update_offer(entry, registry.installs.contains_key(&backend.source));
    let state = match (&installing, &update_to) {
        (Installing::Running { phase, fraction }, _) => RowState::Installing {
            phase: *phase,
            fraction: *fraction,
        },
        (Installing::Failed(message), _) => RowState::Failed(message.clone()),
        (Installing::Idle, Some(version)) => RowState::UpdateReady(version.clone()),
        (Installing::Idle, None) if library.missing_keys.contains(&backend.source) => {
            RowState::NeedsKey
        }
        (Installing::Idle, None) => RowState::Installed,
    };
    let models = model_groups(
        backend
            .models
            .iter()
            .map(|m| (m.name.as_str(), m.role.as_str())),
    );
    Row {
        source: backend.source.clone(),
        name: backend.name.clone(),
        version: backend.version.clone(),
        description: Some(backend.description.clone())
            .filter(|d| !d.is_empty())
            .or_else(|| entry.and_then(|e| e.description.clone()))
            .filter(|d| !d.is_empty()),
        installed: true,
        features: features(&models),
        kinds: models.iter().map(|g| g.kind).collect(),
        runs: if backend_is_online(backend) {
            Runs::Cloud
        } else {
            Runs::Local
        },
        capabilities: backend_capabilities(backend),
        models,
        facts: Facts {
            runs_as: match backend.kind.as_str() {
                "wasm" => RunsAs::Wasm,
                "subprocess" => RunsAs::Subprocess,
                other => RunsAs::Other(other.to_string()),
            },
            needs: backend
                .secrets
                .iter()
                .filter(|s| s.required)
                .map(|s| s.label.clone().unwrap_or_else(|| s.name.clone()))
                .collect(),
            reaches: backend.allowed_hosts.clone(),
            built_for: None,
            license: entry.map_or_default(|e| e.license.clone()),
        },
        frameworks: backend
            .installed_accel
            .iter()
            .map(|accel| Framework {
                accel: accel.clone(),
                download: None,
            })
            .collect(),
        repository: repository_url(&backend.source),
        state,
        installing,
        update_to,
        uninstall_error: registry.uninstall_errors.get(&backend.source).cloned(),
        needs_key: false,
    }
}

/// A backend the "Install manually" sheet previewed, as a row: what the
/// inspector would show of it.
#[must_use]
pub fn preview_row(registry: &RegistryState, backend: &RegistryBackend) -> Row {
    catalog_row(registry, backend)
}

fn catalog_row(registry: &RegistryState, entry: &RegistryBackend) -> Row {
    let installing = installing(registry, &entry.source);
    let compatibility = &entry.compatibility;
    let state = match &installing {
        Installing::Running { phase, fraction } => RowState::Installing {
            phase: *phase,
            fraction: *fraction,
        },
        Installing::Failed(message) => RowState::Failed(message.clone()),
        Installing::Idle if compatibility.needs_client_update => {
            RowState::NeedsNewerApp(entry.min_client.clone())
        }
        Installing::Idle if !compatibility.compatible => {
            RowState::CannotRun(compatibility.reason.clone().filter(|r| !r.is_empty()))
        }
        Installing::Idle => RowState::Available,
    };
    let models = model_groups(
        entry
            .models
            .iter()
            .map(|m| (m.name.as_str(), m.product.role.as_str())),
    );
    Row {
        source: entry.source.clone(),
        name: entry.name.clone(),
        version: entry.version.clone(),
        description: entry.description.clone().filter(|d| !d.is_empty()),
        installed: false,
        features: features(&models),
        kinds: models.iter().map(|g| g.kind).collect(),
        runs: if entry.online {
            Runs::Cloud
        } else {
            Runs::Local
        },
        capabilities: entry_capabilities(entry),
        models,
        facts: facts(entry),
        frameworks: entry
            .builds
            .iter()
            .map(|build| Framework {
                accel: build.accel.clone(),
                download: (!build.on_disk).then_some(build.size),
            })
            .collect(),
        repository: repository_url(&entry.source),
        state,
        installing,
        update_to: None,
        uninstall_error: None,
        needs_key: entry_needs_key(entry),
    }
}

/// The features models of these kinds serve.
fn features(models: &[ModelGroup]) -> Vec<Feature> {
    let stt = models
        .iter()
        .any(|g| matches!(g.kind, ModelKind::Transcription | ModelKind::PostProcessing));
    let tts = models.iter().any(|g| g.kind == ModelKind::Synthesis);
    Feature::ALL
        .into_iter()
        .filter(|feature| match feature {
            Feature::SpeechToText => stt,
            Feature::TextToSpeech => tts,
        })
        .collect()
}

/// Where the install of `source` stands: a catalog backend's, or the
/// "Install manually" sheet's under its [`RegistryState::add_key`].
#[must_use]
pub fn installing(registry: &RegistryState, source: &str) -> Installing {
    if let Some(error) = registry.install_errors.get(source) {
        return Installing::Failed(error.clone());
    }
    let Some(status) = registry.installs.get(source) else {
        return Installing::Idle;
    };
    if let Some(message) = status.failure() {
        return Installing::Failed(message);
    }
    Installing::Running {
        phase: status.phase,
        // reason: a download's bytes as a bar's fraction; cosmetic.
        #[allow(clippy::cast_precision_loss)]
        fraction: status
            .bytes_total
            .filter(|total| *total > 0)
            .map(|total| (status.bytes_done as f32 / total as f32).clamp(0.0, 1.0)),
    }
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
        update(
            &mut registry,
            &mut library,
            Input::InUse {
                source: "whisper".into(),
                in_use: true,
            },
        );
        assert!(!library.pending_in_use, "another backend's answer");
        update(
            &mut registry,
            &mut library,
            Input::InUse {
                source: "kokoro".into(),
                in_use: true,
            },
        );
        assert!(library.pending_in_use);
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

    fn installed(source: &str, name: &str, role: &str, online: bool, key: bool) -> BackendInfo {
        serde_json::from_value(serde_json::json!({
            "source": source,
            "name": name,
            "version": "0.2.1",
            "kind": if online { "wasm" } else { "subprocess" },
            "allowed_hosts": if online { vec!["api.openai.com"] } else { vec![] },
            "installed_accel": if online { vec![] } else { vec!["cuda", "cpu"] },
            "models": [{ "name": format!("{name}-model"), "role": role }],
            "secrets": if key {
                serde_json::json!([{ "name": "api_key", "label": "API key", "required": true }])
            } else {
                serde_json::json!([])
            },
            "options": [],
            "supports": if online { vec!["online"] } else { vec!["gpu", "cpu"] }
        }))
        .expect("an installed backend")
    }

    fn entry(source: &str, name: &str, role: &str, version: &str) -> RegistryBackend {
        serde_json::from_value(serde_json::json!({
            "id": name.to_lowercase(),
            "source": source,
            "version": version,
            "name": name,
            "license": "MIT",
            "kind": "subprocess",
            "contract": "v4",
            "online": false,
            "supports_gpu": true,
            "supports_cpu": true,
            "supports": ["gpu", "cpu"],
            "models": [{
                "name": format!("{name}-model"),
                "role": role,
                "supported_devices": ["cuda", "cpu"]
            }],
            "secrets": [],
            "options": [],
            "compatibility": { "compatible": true },
            "builds": [
                { "accel": "cuda", "size": 57, "on_disk": true },
                { "accel": "cpu", "size": 20 }
            ],
            "installed_version": null,
            "update_available": false
        }))
        .expect("a catalog entry")
    }

    /// The made-up library: Whisper and OpenAI installed, Whisper with an
    /// update; Whisper, Voxtral and Kokoro in the catalog.
    fn library() -> (RegistryState, LibraryState) {
        let mut whisper = entry("github.com/x/whisper", "Whisper", "transcription", "0.2.2");
        whisper.installed_version = Some("0.2.1".into());
        whisper.update_available = true;
        let registry = RegistryState {
            backends: vec![
                whisper,
                entry("github.com/x/voxtral", "Voxtral", "transcription", "0.4.0"),
                entry("github.com/x/kokoro", "Kokoro", "synthesis", "1.1.0"),
            ],
            ..RegistryState::default()
        };
        let library = LibraryState {
            backends: vec![
                installed(
                    "github.com/x/whisper",
                    "Whisper",
                    "transcription",
                    false,
                    false,
                ),
                installed("github.com/x/openai", "OpenAI", "transcription", true, true),
            ],
            ..LibraryState::default()
        };
        (registry, library)
    }

    fn names(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    /// One table: the installed backends first, then the catalog's not
    /// installed; an update offered reads as ready.
    #[test]
    fn the_table_lists_installed_then_available() {
        let (registry, library) = library();
        let rows = rows(&registry, &library);
        assert_eq!(names(&rows), ["Whisper", "OpenAI", "Voxtral", "Kokoro"]);
        assert_eq!(rows[0].state, RowState::UpdateReady("0.2.2".into()));
        assert_eq!(rows[0].facts.license, "MIT", "the catalog's");
        assert_eq!(rows[1].runs, Runs::Cloud);
        assert_eq!(rows[1].facts.needs, ["API key"]);
        assert_eq!(rows[2].state, RowState::Available);
        assert_eq!(
            rows[2].frameworks,
            [
                Framework {
                    accel: "cuda".into(),
                    download: None
                },
                Framework {
                    accel: "cpu".into(),
                    download: Some(20)
                }
            ],
            "a build on disk downloads nothing"
        );
        assert_eq!(rows[3].features, [Feature::TextToSpeech]);
        assert_eq!(updates_ready(&registry, &library), 1);
    }

    /// The search and the filters narrow both halves of the table.
    #[test]
    fn the_filters_narrow_installed_and_available_alike() {
        let (mut registry, mut library) = library();
        update(
            &mut registry,
            &mut library,
            Input::Feature(FeatureFilter::TextToSpeech),
        );
        assert_eq!(names(&rows(&registry, &library)), ["Kokoro"]);
        assert!(filtered(&registry));
        assert_eq!(filters_set(&registry), 1);
        clear_filters(&mut registry);
        update(&mut registry, &mut library, Input::Online(Some(true)));
        assert_eq!(names(&rows(&registry, &library)), ["OpenAI"]);
        clear_filters(&mut registry);
        update(&mut registry, &mut library, Input::Search("VOX".into()));
        assert_eq!(names(&rows(&registry, &library)), ["Voxtral"]);
        update(
            &mut registry,
            &mut library,
            Input::Search("parakeet".into()),
        );
        assert!(rows(&registry, &library).is_empty());
        clear_filters(&mut registry);
        assert!(!filtered(&registry));
    }

    /// The backends read ask which keys are stored, for those that require
    /// one; one missing reads as needing it.
    #[test]
    fn a_backend_without_its_key_says_so() {
        let (mut registry, mut library) = library();
        let backends = std::mem::take(&mut library.backends);
        let effects = update(
            &mut registry,
            &mut library,
            Input::BackendsLoaded(Ok(backends)),
        );
        assert_eq!(
            effects,
            [Effect::ReadKeys {
                source: "github.com/x/openai".into(),
                required: vec!["api_key".into()],
            }]
        );
        update(
            &mut registry,
            &mut library,
            Input::KeysRead {
                source: "github.com/x/openai".into(),
                missing: true,
            },
        );
        let openai = row(&registry, &library, "github.com/x/openai").expect("installed");
        assert_eq!(openai.state, RowState::NeedsKey);
    }

    /// The inspector keeps what was picked when the filters hide its row,
    /// and an install's progress reaches its row.
    #[test]
    fn the_pick_survives_the_filters_and_shows_progress() {
        let (mut registry, mut library) = library();
        update(
            &mut registry,
            &mut library,
            Input::Select(Some("github.com/x/voxtral".into())),
        );
        update(&mut registry, &mut library, Input::Search("kokoro".into()));
        update(
            &mut registry,
            &mut library,
            Input::InstallAccepted {
                source: "github.com/x/voxtral".into(),
                install_id: "i1".into(),
            },
        );
        update(
            &mut registry,
            &mut library,
            Input::Event(RegistryEvent::Progress {
                install_id: "i1".into(),
                source: "github.com/x/voxtral".into(),
                phase: InstallPhase::Downloading,
                bytes_done: Some(38),
                bytes_total: Some(100),
            }),
        );
        let picked = selected(&registry, &library).expect("still picked");
        assert_eq!(
            picked.state,
            RowState::Installing {
                phase: InstallPhase::Downloading,
                fraction: Some(0.38)
            }
        );
        assert!(!picked.installable());
    }

    #[test]
    fn the_catalog_time_is_local_to_the_minute() {
        assert_eq!(catalog_time("not a time"), "not a time");
        assert_eq!(catalog_time("2026-11-25T09:14:00Z").len(), 16);
    }
}
