// SPDX-License-Identifier: GPL-3.0-only
//! The Library page for an app written in another language: a store that
//! holds the installed backends and the registry's catalog, runs the core's
//! logic for them (`pages::library`) on the shared runtime, follows the
//! daemon's install events, and tells the app whenever the page changed.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::daemon::backends::BackendInfo;
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::library::{self as page, Effect, Input, LibraryState};
use super_voice_app_core::rules::backends::{backend_is_online, update_offer};
use super_voice_app_core::state::registry::{FeatureFilter, RefreshOutcome, RegistryState};
use super_voice_shared::registry::events::InstallPhase;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait LibraryObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: LibraryPage);
}

/// Which feature a backend must serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FeatureChoice {
    All,
    SpeechToText,
    TextToSpeech,
}

impl From<FeatureChoice> for FeatureFilter {
    fn from(choice: FeatureChoice) -> Self {
        match choice {
            FeatureChoice::All => Self::All,
            FeatureChoice::SpeechToText => Self::SpeechToText,
            FeatureChoice::TextToSpeech => Self::TextToSpeech,
        }
    }
}

fn feature_choice(filter: FeatureFilter) -> FeatureChoice {
    match filter {
        FeatureFilter::SpeechToText => FeatureChoice::SpeechToText,
        FeatureFilter::TextToSpeech => FeatureChoice::TextToSpeech,
        FeatureFilter::All => FeatureChoice::All,
    }
}

/// Where a backend runs: on this computer, or a service online.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RunsOnChoice {
    All,
    Local,
    Cloud,
}

fn runs_on(online: Option<bool>) -> RunsOnChoice {
    match online {
        None => RunsOnChoice::All,
        Some(false) => RunsOnChoice::Local,
        Some(true) => RunsOnChoice::Cloud,
    }
}

fn online(choice: RunsOnChoice) -> Option<bool> {
    match choice {
        RunsOnChoice::All => None,
        RunsOnChoice::Local => Some(false),
        RunsOnChoice::Cloud => Some(true),
    }
}

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LibraryAction {
    /// Read the installed backends and the catalog: the page was opened.
    Reload,
    /// Refresh the registry's index, then read the catalog.
    RefreshCatalog,
    Search {
        text: String,
    },
    IncludeIncompatible {
        on: bool,
    },
    Feature {
        choice: FeatureChoice,
    },
    RunsOn {
        choice: RunsOnChoice,
    },
    InstalledFeature {
        choice: FeatureChoice,
    },
    InstalledRunsOn {
        choice: RunsOnChoice,
    },
    Install {
        source: String,
    },
    Update {
        source: String,
    },
    AskUninstall {
        source: String,
    },
    CancelUninstall,
    Uninstall {
        source: String,
    },
}

/// An installed backend.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InstalledRow {
    pub source: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Its models are served online.
    pub online: bool,
    pub models: u32,
    /// The version an update would bring, when one is offered.
    pub update_to: Option<String>,
    /// An install of it (an update) is running.
    pub installing: bool,
    /// Why its last uninstall failed.
    pub uninstall_error: Option<String>,
}

/// Where an install stands.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum InstallStanding {
    /// Not installing: Install.
    Idle,
    Running {
        phase: InstallStep,
        /// How far, 0 to 1, when the size is known.
        fraction: Option<f32>,
    },
    /// It failed, or didn't start: why.
    Failed { message: String },
}

/// A step of an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InstallStep {
    Resolving,
    Downloading,
    Verifying,
    Extracting,
    Installing,
    Rescanning,
}

/// One backend from the catalog.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CatalogRow {
    pub source: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub online: bool,
    pub models: u32,
    /// It runs on this computer. When not, why.
    pub compatible: bool,
    pub incompatible_reason: Option<String>,
    /// It needs a newer Super Voice, this one.
    pub needs_version: Option<String>,
    pub install: InstallStanding,
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LibraryPage {
    pub installed: Vec<InstalledRow>,
    pub installed_total: u32,
    pub installed_feature: FeatureChoice,
    pub installed_runs_on: RunsOnChoice,
    /// The catalog has been read.
    pub catalog_loaded: bool,
    /// Why the catalog couldn't be read.
    pub catalog_error: Option<String>,
    /// A refresh failed, so the list shown may be out of date.
    pub catalog_stale: bool,
    pub catalog: Vec<CatalogRow>,
    pub search: String,
    pub include_incompatible: bool,
    pub feature: FeatureChoice,
    pub runs_on: RunsOnChoice,
    /// The backend whose uninstall waits for the user to confirm.
    pub pending_uninstall: Option<String>,
}

#[derive(Default)]
struct Held {
    registry: RegistryState,
    library: LibraryState,
    backends: Vec<BackendInfo>,
}

/// The Library page's store.
#[derive(uniffi::Object)]
pub struct LibraryStore {
    held: Mutex<Held>,
    observer: Arc<dyn LibraryObserver>,
}

#[uniffi::export]
impl LibraryStore {
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn LibraryObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    pub fn page(&self) -> LibraryPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: LibraryAction) {
        let input = match action {
            LibraryAction::Reload => {
                Arc::clone(&self).reload_backends();
                Input::Refresh { index: false }
            }
            LibraryAction::RefreshCatalog => Input::Refresh { index: true },
            LibraryAction::Search { text } => Input::Search(text),
            LibraryAction::IncludeIncompatible { on } => Input::IncludeIncompatible(on),
            LibraryAction::Feature { choice } => Input::Feature(choice.into()),
            LibraryAction::RunsOn { choice } => Input::Online(online(choice)),
            LibraryAction::InstalledFeature { choice } => Input::InstalledFeature(choice.into()),
            LibraryAction::InstalledRunsOn { choice } => Input::InstalledOnline(online(choice)),
            LibraryAction::Install { source } => Input::Install(source),
            LibraryAction::Update { source } => Input::Update(source),
            LibraryAction::AskUninstall { source } => Input::AskUninstall(source),
            LibraryAction::CancelUninstall => Input::CancelUninstall,
            LibraryAction::Uninstall { source } => Input::Uninstall(source),
        };
        self.dispatch(input);
    }
}

impl LibraryStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn changed(&self) {
        let page = snapshot(&self.lock());
        self.observer.changed(page);
    }

    /// Read the installed backends.
    fn reload_backends(self: Arc<Self>) {
        RUNTIME.spawn(async move {
            // A failed read keeps the list as it was: the next event or
            // visit reads it again.
            if let Ok(backends) = super_voice_app_core::daemon::client::list_backends().await {
                self.lock().backends = backends;
            }
            self.changed();
        });
    }

    fn dispatch(self: Arc<Self>, input: Input) {
        let effects = {
            let mut held = self.lock();
            let Held {
                registry, library, ..
            } = &mut *held;
            page::update(registry, library, input)
        };
        self.changed();
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
    }

    fn run(self: Arc<Self>, effect: Effect) {
        let work: Option<std::pin::Pin<Box<dyn Future<Output = Input> + Send>>> = match effect {
            Effect::FetchCatalog { refresh_index } => {
                Some(Box::pin(page::run_catalog(refresh_index)))
            }
            Effect::Install(source) => Some(Box::pin(page::run_install(source))),
            Effect::Update(source) => Some(Box::pin(page::run_update(source))),
            Effect::Uninstall(source) => Some(Box::pin(page::run_uninstall(source))),
            Effect::ReloadBackends => {
                Arc::clone(&self).reload_backends();
                None
            }
            Effect::ReloadCatalogs => {
                Arc::clone(&self).reload_backends();
                Some(Box::pin(page::run_catalog(false)))
            }
            // This app has no framework builds or add sheet to tell.
            Effect::Installed { .. } | Effect::InstallFailed { .. } => None,
        };
        if let Some(work) = work {
            RUNTIME.spawn(async move {
                let input = work.await;
                self.dispatch(input);
            });
        }
    }
}

/// Hand the daemon's install events to `store` until it is dropped, and read
/// everything again when the daemon is back.
fn follow(store: Weak<LibraryStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            match event {
                Event::Install(event) => store.dispatch(Input::Event(event)),
                Event::Connected => store.send(LibraryAction::Reload),
                _ => {}
            }
        }
    });
}

fn snapshot(held: &Held) -> LibraryPage {
    let registry = &held.registry;
    let filters = &held.library.installed_filters;
    let catalog = registry.by_source();
    let installed = held
        .backends
        .iter()
        .filter(|b| page::installed_matches(b, filters))
        .map(|b| InstalledRow {
            source: b.source.clone(),
            name: b.name.clone(),
            version: b.version.clone(),
            description: b.description.clone(),
            online: backend_is_online(b),
            models: count(b.models.len()),
            update_to: update_offer(
                catalog.get(b.source.as_str()).copied(),
                registry.installs.contains_key(&b.source),
            ),
            installing: registry.installs.contains_key(&b.source),
            uninstall_error: registry.uninstall_errors.get(&b.source).cloned(),
        })
        .collect();
    let catalog_rows = page::catalog_entries(registry, &held.backends)
        .into_iter()
        .map(|entry| CatalogRow {
            source: entry.source.clone(),
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone().filter(|d| !d.is_empty()),
            online: entry.online,
            models: count(entry.models.len()),
            compatible: entry.compatibility.compatible,
            incompatible_reason: entry.compatibility.reason.clone(),
            needs_version: entry
                .compatibility
                .needs_client_update
                .then(|| entry.min_client.clone())
                .flatten(),
            install: install_standing(registry, &entry.source),
        })
        .collect();
    LibraryPage {
        installed,
        installed_total: count(held.backends.len()),
        installed_feature: feature_choice(filters.feature),
        installed_runs_on: runs_on(filters.online),
        catalog_loaded: registry.last_refresh.is_some(),
        catalog_error: match &registry.last_refresh {
            Some(RefreshOutcome::Failed(e)) => Some(e.clone()),
            _ => None,
        },
        catalog_stale: matches!(registry.last_refresh, Some(RefreshOutcome::Stale)),
        catalog: catalog_rows,
        search: registry.filters.search.clone(),
        include_incompatible: registry.filters.include_incompatible,
        feature: feature_choice(registry.filters.feature),
        runs_on: runs_on(registry.filters.online),
        pending_uninstall: held.library.pending_uninstall.clone(),
    }
}

fn install_standing(registry: &RegistryState, source: &str) -> InstallStanding {
    if let Some(error) = registry.install_errors.get(source) {
        return InstallStanding::Failed {
            message: error.clone(),
        };
    }
    let Some(status) = registry.installs.get(source) else {
        return InstallStanding::Idle;
    };
    if let Some(message) = status.failure() {
        return InstallStanding::Failed { message };
    }
    InstallStanding::Running {
        phase: match status.phase {
            InstallPhase::Resolving => InstallStep::Resolving,
            InstallPhase::Downloading => InstallStep::Downloading,
            InstallPhase::Verifying => InstallStep::Verifying,
            InstallPhase::Extracting => InstallStep::Extracting,
            InstallPhase::Installing => InstallStep::Installing,
            InstallPhase::Rescanning => InstallStep::Rescanning,
        },
        // reason: a download's bytes as a bar's fraction; cosmetic.
        #[allow(clippy::cast_precision_loss)]
        fraction: status
            .bytes_total
            .filter(|total| *total > 0)
            .map(|total| (status.bytes_done as f32 / total as f32).clamp(0.0, 1.0)),
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
