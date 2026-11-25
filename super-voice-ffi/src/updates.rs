// SPDX-License-Identifier: GPL-3.0-only
//! The Updates page for an app written in another language: a store that
//! holds the page's state, runs the core's logic for it (`pages::updates`) and
//! the installer's run, follows the daemon's update events, and tells the app
//! whenever the page changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use chrono::DateTime;
use futures_util::StreamExt;
use super_voice_app_core::events::{self, Event};
use super_voice_app_core::pages::updates::{self as page, Effect, Input};
use super_voice_app_core::state::update::{RunPhase, UpdateNotice, UpdateState};
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait UpdatesObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: UpdatesPage);
}

/// What the user did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UpdatesAction {
    /// Read the status and settings: the page was opened.
    Reload,
    CheckNow,
    AutoCheckToggled {
        on: bool,
    },
    BetaToggled {
        on: bool,
    },
    StartUpdate,
    CancelUpdate,
    DismissRun,
}

/// Where an update run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UpdatePhase {
    FetchingInstaller,
    Resolve,
    Download,
    Verify,
    Stage,
    WaitingAuth,
    Install,
    PostInstall,
}

/// The update section's content.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum UpdatePanel {
    /// An update to offer. Without an installer for this computer, the
    /// terminal command to run instead.
    Offered {
        tag: String,
        can_start: bool,
        install_command: Option<String>,
    },
    Running {
        phase: UpdatePhase,
        /// How far the download is, 0 to 1, while there is one.
        fraction: Option<f32>,
        cancellable: bool,
    },
    /// Installed. The app itself was updated, so it needs a restart.
    Done {
        restart: bool,
        error: Option<String>,
    },
    /// The run failed: why, and the command to install by hand, unless the
    /// user only declined the password prompt.
    Failed {
        error: Option<String>,
        install_command: Option<String>,
    },
}

/// Check now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CheckNow {
    /// It may be pressed.
    pub enabled: bool,
    /// A check is running.
    pub checking: bool,
}

/// The beta-updates switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct BetaSwitch {
    pub on: bool,
    /// It waits for its write and the re-check after it.
    pub pending: bool,
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UpdatesPage {
    pub loaded: bool,
    /// The daemon predates updates.
    pub unsupported: bool,
    pub latest_version: Option<String>,
    /// When the daemon last checked, as seconds since 1970; `None` before
    /// it has, or for what isn't a time.
    pub checked_at: Option<i64>,
    pub last_check_error: Option<String>,
    pub check: CheckNow,
    /// Whether the daemon checks on its own; `None` until read.
    pub auto_check: Option<bool>,
    /// The beta switch, when it shows.
    pub beta: Option<BetaSwitch>,
    /// The update section, when there is one to show.
    pub panel: Option<UpdatePanel>,
    /// What to tell the user, in the user's language.
    pub notice: Option<String>,
    /// Whether the window header shows "Update available", which opens this
    /// page: while an update is offered and none is running.
    pub header_badge: bool,
}

#[derive(Default)]
struct Held {
    loaded: bool,
    state: UpdateState,
}

/// The Updates page's store.
#[derive(uniffi::Object)]
pub struct UpdatesStore {
    held: Mutex<Held>,
    observer: Arc<dyn UpdatesObserver>,
}

#[uniffi::export]
impl UpdatesStore {
    /// A store that tells `observer` what changes, and follows the daemon's
    /// update events.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn UpdatesObserver>) -> Arc<Self> {
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    pub fn page(&self) -> UpdatesPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: UpdatesAction) {
        let input = match action {
            UpdatesAction::Reload => {
                self.reload();
                return;
            }
            UpdatesAction::CheckNow => Input::CheckNow,
            UpdatesAction::AutoCheckToggled { on } => Input::AutoCheckToggled(on),
            UpdatesAction::BetaToggled { on } => Input::BetaOptinToggled(on),
            UpdatesAction::StartUpdate => Input::StartUpdate,
            UpdatesAction::CancelUpdate => Input::CancelUpdate,
            UpdatesAction::DismissRun => Input::DismissRun,
        };
        self.dispatch(input);
    }
}

impl UpdatesStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reload(self: Arc<Self>) {
        RUNTIME.spawn(async move {
            let (status, auto_check) =
                tokio::join!(page::run_status(), page::run_load_auto_check());
            Arc::clone(&self).dispatch(auto_check);
            self.lock().loaded = true;
            self.dispatch(status);
        });
    }

    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let effects = page::update(&mut held.state, input);
            (snapshot(&held), effects)
        };
        self.observer.changed(page);
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
    }

    fn run(self: Arc<Self>, effect: Effect) {
        if let Effect::StartRun { asset, tag } = effect {
            let (run, handle) = futures_util::stream::abortable(
                super_voice_app_core::updater::run_update_stream(asset, tag),
            );
            self.lock().state.run_abort = Some(handle);
            RUNTIME.spawn(async move {
                let mut run = std::pin::pin!(run);
                while let Some(event) = run.next().await {
                    Arc::clone(&self).dispatch(Input::RunEvent(event));
                }
            });
            return;
        }
        RUNTIME.spawn(async move {
            let input = match effect {
                Effect::ReadStatus => page::run_status().await,
                Effect::Check => page::run_check().await,
                Effect::SaveAutoCheck(on) => page::run_save_auto_check(on).await,
                Effect::SaveBetaOptin(on) => page::run_save_beta_optin(on).await,
                Effect::StartRun { .. } => return,
            };
            self.dispatch(input);
        });
    }
}

/// Follow the daemon: an update it found, a switch another client flipped,
/// or the daemon back after a restart.
fn follow(store: Weak<UpdatesStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let input = match (&event, events::setting_changed(&event)) {
                (Event::Connected, _) | (_, Some("update_beta_optin")) => Input::Available,
                (_, Some("update_check_enabled")) => page::run_load_auto_check().await,
                (Event::Status(status), _)
                    if status.data.get("status").and_then(|s| s.as_str())
                        == Some("update_available") =>
                {
                    Input::Available
                }
                _ => continue,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            if store.lock().loaded {
                store.dispatch(input);
            }
        }
    });
}

fn snapshot(held: &Held) -> UpdatesPage {
    let state = &held.state;
    let status = state.status.as_ref();
    let tag = || {
        status
            .and_then(|s| s.latest_version.clone())
            .unwrap_or_else(|| super_voice_app_core::i18n::text("the-latest-version", &[]))
    };
    let panel = state.update_section_visible().then(|| match &state.run {
        None => {
            let tag = tag();
            UpdatePanel::Offered {
                can_start: state.can_start_update(),
                install_command: (!state.can_start_update()).then(|| page::install_command(&tag)),
                tag,
            }
        }
        Some(run) => match run.phase {
            RunPhase::Done => UpdatePanel::Done {
                restart: run.completed_components.iter().any(|c| c == "app"),
                error: run.error.clone(),
            },
            RunPhase::Failed => UpdatePanel::Failed {
                error: run.error.clone(),
                install_command: (!run.was_authorization_declined())
                    .then(|| page::install_command(&tag())),
            },
            phase => UpdatePanel::Running {
                phase: match phase {
                    RunPhase::FetchingInstaller => UpdatePhase::FetchingInstaller,
                    RunPhase::Resolve => UpdatePhase::Resolve,
                    RunPhase::Download => UpdatePhase::Download,
                    RunPhase::Verify => UpdatePhase::Verify,
                    RunPhase::Stage => UpdatePhase::Stage,
                    RunPhase::WaitingAuth => UpdatePhase::WaitingAuth,
                    RunPhase::Install => UpdatePhase::Install,
                    RunPhase::PostInstall | RunPhase::Done | RunPhase::Failed => {
                        UpdatePhase::PostInstall
                    }
                },
                // reason: a download's bytes as a bar's fraction; cosmetic.
                #[allow(clippy::cast_precision_loss)]
                fraction: matches!(phase, RunPhase::FetchingInstaller | RunPhase::Download)
                    .then(|| (run.bytes_done as f32 / run.bytes_total.max(1) as f32).max(0.05)),
                cancellable: run.cancellable(),
            },
        },
    });
    UpdatesPage {
        loaded: held.loaded,
        unsupported: state.unsupported,
        latest_version: status.and_then(|s| s.latest_version.clone()),
        checked_at: status
            .and_then(|s| s.checked_at.as_deref())
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.timestamp()),
        last_check_error: status.and_then(|s| s.last_check_error.clone()),
        check: CheckNow {
            enabled: state.can_check_now(),
            checking: state.checking,
        },
        auto_check: state.auto_check_enabled,
        beta: state.beta_optin_shown().then(|| BetaSwitch {
            on: state.beta_optin.unwrap_or(false),
            pending: state.beta_pending,
        }),
        panel,
        notice: state.notice.as_ref().map(notice_text),
        header_badge: state.header_badge_visible(),
    }
}

/// What the page tells the user, in the settings app's words.
fn notice_text(notice: &UpdateNotice) -> String {
    let (key, error) = match notice {
        UpdateNotice::StatusUnread(e) => ("update-status-could-not-read", e),
        UpdateNotice::SettingUnsaved(e) => ("update-setting-could-not-save", e),
    };
    crate::text_with(key, HashMap::from([("error".to_string(), error.clone())]))
}

/// The version this app is.
#[must_use]
#[uniffi::export]
pub fn app_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}
