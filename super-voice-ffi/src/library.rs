// SPDX-License-Identifier: GPL-3.0-only
//! The Library page for an app written in another language: a store that
//! holds the installed backends and the registry's catalog, runs the core's
//! logic for them (`pages::library`) on the shared runtime, follows the
//! daemon's install events, and tells the app whenever the page changed.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::daemon::backends::BackendInfo;
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::add_backend::{self as add, RunsAs};
use super_voice_app_core::pages::library::{self as page, Effect, Input, LibraryState};
use super_voice_app_core::rules::backends::{
    Capabilities, ModelKind, backend_capabilities, backend_is_online, entry_capabilities,
    entry_needs_key, model_groups, repository_url, update_offer,
};
use super_voice_app_core::state::registry::{
    AddSource, FeatureFilter, PreviewState, RefreshOutcome, RegistryState, RoleFilter,
};
use super_voice_shared::registry::RegistryBackend;
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

/// Which kind of speech-to-text model a backend must serve: the filter under
/// speech to text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KindChoice {
    All,
    Transcription,
    PostProcessing,
}

impl From<KindChoice> for RoleFilter {
    fn from(choice: KindChoice) -> Self {
        match choice {
            KindChoice::All => Self::All,
            KindChoice::Transcription => Self::Transcription,
            KindChoice::PostProcessing => Self::PostProcessing,
        }
    }
}

fn kind_choice(filter: RoleFilter) -> KindChoice {
    match filter {
        RoleFilter::All => KindChoice::All,
        RoleFilter::Transcription => KindChoice::Transcription,
        RoleFilter::PostProcessing => KindChoice::PostProcessing,
    }
}

/// Where the "Install manually" sheet installs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AddSourceChoice {
    /// A git repository's latest release.
    Repository,
    /// A folder on this computer.
    Folder,
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
    /// The Kind filter, shown under speech to text.
    Kind {
        choice: KindChoice,
    },
    InstalledKind {
        choice: KindChoice,
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
    /// Open the "Install manually" sheet.
    OpenAdd,
    /// Close it. What it holds stays for the next time.
    CloseAdd,
    AddSource {
        choice: AddSourceChoice,
    },
    /// The repository URL field changed.
    AddUrl {
        text: String,
    },
    /// The folder field changed.
    AddFolder {
        path: String,
    },
    /// A folder was chosen in the app's own folder picker: it is checked at
    /// once.
    AddFolderChosen {
        path: String,
    },
    /// Check: read the source and preview what it would install.
    AddCheck,
    /// Install what the preview shows.
    AddInstall,
}

/// One capability chip on a backend's card.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BackendCapability {
    /// It runs models on a GPU.
    Gpu,
    /// It runs models on the CPU.
    Cpu,
    /// It sends audio to a service online: these hosts, and another URL the
    /// user set, when `user_url`. The URL itself is never shown.
    Cloud { hosts: Vec<String>, user_url: bool },
    /// It hears the user's dictation context.
    Contexts,
    /// It speaks in voices the user cloned.
    VoiceCloning,
}

/// What a model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LibraryModelKind {
    Transcription,
    PostProcessing,
    Synthesis,
}

impl From<ModelKind> for LibraryModelKind {
    fn from(kind: ModelKind) -> Self {
        match kind {
            ModelKind::Transcription => Self::Transcription,
            ModelKind::PostProcessing => Self::PostProcessing,
            ModelKind::Synthesis => Self::Synthesis,
        }
    }
}

/// The models of one kind a backend ships, in pipeline order.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibraryModelGroup {
    pub kind: LibraryModelKind,
    pub names: Vec<String>,
}

/// An installed backend.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InstalledRow {
    pub source: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Its models are served online.
    pub online: bool,
    pub capabilities: Vec<BackendCapability>,
    pub model_groups: Vec<LibraryModelGroup>,
    /// Its repository's web page.
    pub repository: String,
    /// The version an update would bring, when one is offered.
    pub update_to: Option<String>,
    /// Its update: under way (its step), failed (why), or none.
    pub update: InstallStanding,
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
    pub capabilities: Vec<BackendCapability>,
    pub model_groups: Vec<LibraryModelGroup>,
    /// Its repository's web page.
    pub repository: String,
    /// It needs an API key before it runs.
    pub needs_key: bool,
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
    pub installed_kind: KindChoice,
    /// The catalog has been read.
    pub catalog_loaded: bool,
    /// Why the catalog couldn't be read.
    pub catalog_error: Option<String>,
    /// A refresh failed, so the list shown may be out of date.
    pub catalog_stale: bool,
    /// When the registry's index was made, as the daemon wrote it (RFC 3339).
    pub catalog_updated: Option<String>,
    pub catalog: Vec<CatalogRow>,
    pub search: String,
    pub include_incompatible: bool,
    pub feature: FeatureChoice,
    pub runs_on: RunsOnChoice,
    pub kind: KindChoice,
    /// The uninstall that waits for the user to confirm.
    pub pending_uninstall: Option<UninstallQuestion>,
    /// The "Install manually" sheet.
    pub add: AddSheet,
}

/// The question asked before uninstalling a backend.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UninstallQuestion {
    pub source: String,
    pub name: String,
    /// A stage runs it, so uninstalling it stops it first.
    pub in_use: bool,
}

/// The "Install manually" sheet: a source to point at, Check to read it, the
/// preview of what it would install, and Install.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AddSheet {
    /// It is up.
    pub open: bool,
    pub source: AddSourceChoice,
    pub url: String,
    pub folder: String,
    pub preview: AddPreview,
    /// The install it started: under way, failed, or none.
    pub install: InstallStanding,
    /// The name of the backend Install would install, when it can: one was
    /// previewed, this computer can run it, and it isn't installing.
    pub installable: Option<String>,
}

/// What the sheet's preview shows.
// reason: one preview at a time, handed over by value to the app
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum AddPreview {
    /// Nothing checked yet.
    Empty,
    /// Reading this source.
    Checking {
        what: String,
    },
    /// It couldn't be read: the daemon's words, which name it and say what
    /// to do.
    Failed {
        message: String,
    },
    Ready {
        backend: AddPreviewBackend,
    },
}

/// How a backend runs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum AddRunsAs {
    /// A WASM component, sandboxed.
    Wasm,
    /// A native subprocess.
    Subprocess,
    /// A kind this app has no name for, as its manifest spells it.
    Other { kind: String },
}

/// One model a previewed backend serves.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AddPreviewModel {
    pub name: String,
    pub kind: LibraryModelKind,
    /// It runs on a GPU, on the CPU, online.
    pub gpu: bool,
    pub cpu: bool,
    pub online: bool,
}

/// The backend a source would install, and what installing it agrees to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AddPreviewBackend {
    pub source: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    /// When it can't run on this computer, why.
    pub incompatible: Option<String>,
    pub capabilities: Vec<BackendCapability>,
    pub runs_as: AddRunsAs,
    /// The secrets it can't run without, by label.
    pub needs: Vec<String>,
    /// The hosts it reaches; none means it runs offline.
    pub reaches: Vec<String>,
    /// The build this computer would get: its target and frameworks.
    pub built_for: Option<String>,
    pub license: String,
    pub models: Vec<AddPreviewModel>,
}

#[derive(Default)]
struct Held {
    registry: RegistryState,
    library: LibraryState,
    backends: Vec<BackendInfo>,
    /// The "Install manually" sheet is up.
    adding: bool,
    /// A stage runs the backend whose uninstall waits to be confirmed.
    in_use: bool,
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
            LibraryAction::Kind { choice } => Input::Role(choice.into()),
            LibraryAction::InstalledKind { choice } => Input::InstalledRole(choice.into()),
            LibraryAction::Install { source } => Input::Install(source),
            LibraryAction::Update { source } => Input::Update(source),
            LibraryAction::AskUninstall { source } => {
                Arc::clone(&self).read_in_use(source.clone());
                Input::AskUninstall(source)
            }
            LibraryAction::CancelUninstall => Input::CancelUninstall,
            LibraryAction::Uninstall { source } => Input::Uninstall(source),
            LibraryAction::OpenAdd => return self.show_add(true),
            LibraryAction::CloseAdd => return self.show_add(false),
            LibraryAction::AddSource { choice } => {
                return self.dispatch_add(add::Input::Source(match choice {
                    AddSourceChoice::Repository => AddSource::Repository,
                    AddSourceChoice::Folder => AddSource::Folder,
                }));
            }
            LibraryAction::AddUrl { text } => return self.dispatch_add(add::Input::Url(text)),
            LibraryAction::AddFolder { path } => {
                return self.dispatch_add(add::Input::Folder(path));
            }
            LibraryAction::AddFolderChosen { path } => {
                return self.dispatch_add(add::Input::Picked(Some(path)));
            }
            LibraryAction::AddCheck => return self.dispatch_add(add::Input::Check),
            LibraryAction::AddInstall => return self.dispatch_add(add::Input::Install),
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

    /// Put the sheet up, or take it down.
    fn show_add(&self, open: bool) {
        self.lock().adding = open;
        self.changed();
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

    /// Read whether a stage runs `source`, for the uninstall question. Not
    /// until the answer comes does the question say so.
    fn read_in_use(self: Arc<Self>, source: String) {
        self.lock().in_use = false;
        RUNTIME.spawn(async move {
            let in_use = page::in_use(&source).await;
            {
                let mut held = self.lock();
                if held.library.pending_uninstall.as_deref() != Some(source.as_str()) {
                    return;
                }
                held.in_use = in_use;
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
            // The sheet's work is done once what it previewed is installed.
            Effect::Installed { source } => {
                if self.lock().adding {
                    Arc::clone(&self).dispatch_add(add::Input::Installed(source));
                }
                None
            }
            // This app has no framework builds to tell.
            Effect::InstallFailed { .. } => None,
        };
        if let Some(work) = work {
            RUNTIME.spawn(async move {
                let input = work.await;
                self.dispatch(input);
            });
        }
    }

    /// The "Install manually" sheet's `input`.
    fn dispatch_add(self: Arc<Self>, input: add::Input) {
        let effects = add::update(&mut self.lock().registry, input);
        for effect in effects {
            match effect {
                // The app opens its own folder picker, and hands over what
                // was chosen (`AddFolderChosen`).
                add::Effect::PickFolder => {}
                add::Effect::Preview(target) => {
                    let store = Arc::clone(&self);
                    RUNTIME.spawn(async move {
                        let input = add::run_preview(target).await;
                        store.dispatch_add(input);
                    });
                }
                add::Effect::Install(target) => {
                    let store = Arc::clone(&self);
                    RUNTIME.spawn(async move {
                        let input = add::run_install(target).await;
                        store.dispatch(input);
                    });
                }
                add::Effect::Close => self.lock().adding = false,
            }
        }
        self.changed();
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
            capabilities: capabilities(backend_capabilities(b)),
            model_groups: groups(b.models.iter().map(|m| (m.name.as_str(), m.role.as_str()))),
            repository: repository_url(&b.source),
            update_to: update_offer(
                catalog.get(b.source.as_str()).copied(),
                registry.installs.contains_key(&b.source),
            ),
            update: install_standing(registry, &b.source),
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
            capabilities: capabilities(entry_capabilities(entry)),
            model_groups: entry_groups(entry),
            repository: repository_url(&entry.source),
            needs_key: entry_needs_key(entry),
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
        installed_kind: kind_choice(filters.role),
        catalog_loaded: registry.last_refresh.is_some(),
        catalog_error: match &registry.last_refresh {
            Some(RefreshOutcome::Failed(e)) => Some(e.clone()),
            _ => None,
        },
        catalog_stale: matches!(registry.last_refresh, Some(RefreshOutcome::Stale)),
        catalog_updated: registry.generated_at.clone(),
        catalog: catalog_rows,
        search: registry.filters.search.clone(),
        include_incompatible: registry.filters.include_incompatible,
        feature: feature_choice(registry.filters.feature),
        runs_on: runs_on(registry.filters.online),
        kind: kind_choice(registry.filters.role),
        pending_uninstall: held.library.pending_uninstall.as_ref().map(|source| {
            UninstallQuestion {
                source: source.clone(),
                name: held
                    .backends
                    .iter()
                    .find(|b| &b.source == source)
                    .map_or_else(|| source.clone(), |b| b.name.clone()),
                in_use: held.in_use,
            }
        }),
        add: add_sheet(held),
    }
}

/// The sheet, as the registry holds it.
fn add_sheet(held: &Held) -> AddSheet {
    let registry = &held.registry;
    let key = registry.add_key();
    AddSheet {
        open: held.adding,
        source: match registry.add_source {
            AddSource::Repository => AddSourceChoice::Repository,
            AddSource::Folder => AddSourceChoice::Folder,
        },
        url: registry.custom_repo_input.clone(),
        folder: registry.add_folder.clone(),
        preview: match &registry.add_preview {
            PreviewState::Empty => AddPreview::Empty,
            PreviewState::Checking(what) => AddPreview::Checking { what: what.clone() },
            PreviewState::Failed(message) => AddPreview::Failed {
                message: message.clone(),
            },
            PreviewState::Ready(backend) => AddPreview::Ready {
                backend: previewed(backend),
            },
        },
        install: if key.is_empty() {
            InstallStanding::Idle
        } else {
            install_standing(registry, &key)
        },
        installable: add::installable(registry).map(|b| b.name.clone()),
    }
}

fn previewed(backend: &RegistryBackend) -> AddPreviewBackend {
    let facts = add::facts(backend);
    AddPreviewBackend {
        source: backend.source.clone(),
        name: backend.name.clone(),
        version: backend.version.clone(),
        description: backend.description.clone().filter(|d| !d.is_empty()),
        // An empty reason is no reason: the app says it in its own words.
        incompatible: (!backend.compatibility.compatible)
            .then(|| backend.compatibility.reason.clone().unwrap_or_default()),
        capabilities: capabilities(entry_capabilities(backend)),
        runs_as: match facts.runs_as {
            RunsAs::Wasm => AddRunsAs::Wasm,
            RunsAs::Subprocess => AddRunsAs::Subprocess,
            RunsAs::Other(kind) => AddRunsAs::Other { kind },
        },
        needs: facts.needs,
        reaches: facts.reaches,
        built_for: facts.built_for,
        license: facts.license,
        models: backend
            .models
            .iter()
            .map(|m| {
                let runs = |device: &str| m.supported_devices.iter().any(|d| d == device);
                AddPreviewModel {
                    name: m.name.clone(),
                    kind: ModelKind::of(&m.product.role).into(),
                    gpu: runs("gpu"),
                    cpu: runs("cpu"),
                    online: runs("none"),
                }
            })
            .collect(),
    }
}

/// A backend's chips, in a card's order.
fn capabilities(caps: Capabilities) -> Vec<BackendCapability> {
    let mut chips = Vec::new();
    if caps.compute.gpu {
        chips.push(BackendCapability::Gpu);
    }
    if caps.compute.cpu {
        chips.push(BackendCapability::Cpu);
    }
    if let Some(egress) = caps.cloud {
        chips.push(BackendCapability::Cloud {
            hosts: egress.hosts,
            user_url: egress.user_url,
        });
    }
    if caps.contexts {
        chips.push(BackendCapability::Contexts);
    }
    if caps.voice_cloning {
        chips.push(BackendCapability::VoiceCloning);
    }
    chips
}

fn groups<'a>(models: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<LibraryModelGroup> {
    model_groups(models)
        .into_iter()
        .map(|group| LibraryModelGroup {
            kind: group.kind.into(),
            names: group.names,
        })
        .collect()
}

fn entry_groups(entry: &RegistryBackend) -> Vec<LibraryModelGroup> {
    groups(
        entry
            .models
            .iter()
            .map(|m| (m.name.as_str(), m.product.role.as_str())),
    )
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
