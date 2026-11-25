// SPDX-License-Identifier: GPL-3.0-only
//! The Updates page: the daemon's status and its checks, the two settings
//! switches, and the run that downloads and starts the installer.
//!
//! Every state change is a method on [`UpdateState`]; this says which work
//! follows from each input. Relaunching the app after an update, and opening
//! the page from the header's badge, are each app's own.

use super_voice_shared::models::self_update::{InstallerAsset, SelfUpdateStatus};

use crate::daemon::client::v1::settings::{update_beta_optin, update_check_enabled};
use crate::daemon::client::v1::update as client;
use crate::state::update::{RunOutcome, UpdateRun, UpdateState};
use crate::updater::UpdateRunEvent;

/// How a beta-opt-in switch ended. Three cases because each leaves the page
/// somewhere different: only a failed *write* may snap the switch back. A
/// failed re-check means the setting did change and only the candidate
/// version is unknown, so reverting the switch would misreport the daemon.
#[derive(Debug, Clone)]
pub enum BetaOptinOutcome {
    /// Saved, and the re-check returned a fresh status (`None`: the daemon
    /// predates `/v1/update`).
    Applied(Option<SelfUpdateStatus>),
    /// The write itself failed; nothing changed daemon-side.
    WriteFailed(String),
    /// Saved, but the follow-up re-check failed. The switch stands; only the
    /// banner reports it.
    CheckFailed(String),
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The daemon's status (`None`: it predates updates).
    StatusLoaded(Option<SelfUpdateStatus>),
    /// It couldn't be read.
    StatusError(String),
    /// Check now was pressed, or the first check is due.
    CheckNow,
    /// The daemon said whether it checks on its own.
    AutoCheckLoaded(bool),
    /// That switch was flipped.
    AutoCheckToggled(bool),
    /// The beta-updates switch was flipped.
    BetaOptinToggled(bool),
    /// Its write and the re-check after it settled.
    BetaOptinApplied {
        enabled: bool,
        outcome: BetaOptinOutcome,
    },
    /// A setting couldn't be stored.
    SettingError(String),
    /// The daemon says an update is available: read the status again.
    Available,
    /// Update was pressed.
    StartUpdate,
    /// Cancel was pressed during a run.
    CancelUpdate,
    /// A finished run's panel was closed.
    DismissRun,
    /// The run went on.
    RunEvent(UpdateRunEvent),
}

/// What the app should do.
#[derive(Debug, Clone)]
pub enum Effect {
    /// Read the status: [`run_status`].
    ReadStatus,
    /// Ask the daemon to check now: [`run_check`].
    Check,
    /// Store whether the daemon checks on its own: [`run_save_auto_check`].
    SaveAutoCheck(bool),
    /// Store the beta switch, then check: [`run_save_beta_optin`].
    SaveBetaOptin(bool),
    /// Download and start the installer: run
    /// [`crate::updater::run_update_stream`] as an abortable stream, keep its
    /// abort handle in [`UpdateState::run_abort`], and hand each event back
    /// as [`Input::RunEvent`].
    StartRun { asset: InstallerAsset, tag: String },
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut UpdateState, input: Input) -> Vec<Effect> {
    match input {
        Input::StatusLoaded(status) => {
            state.apply_status_loaded(status);
            first_check(state)
        }
        Input::StatusError(e) => {
            log::warn!("Update status error: {e}");
            state.apply_status_error(&e);
            Vec::new()
        }
        Input::SettingError(e) => {
            log::warn!("Update setting save failed: {e}");
            state.apply_setting_error(&e);
            Vec::new()
        }
        Input::CheckNow => {
            if state.checking {
                return Vec::new();
            }
            state.checking = true;
            state.notice = None;
            vec![Effect::Check]
        }
        Input::AutoCheckLoaded(enabled) => {
            state.auto_check_enabled = Some(enabled);
            // The status may have landed first, when whether to ask could not
            // be told yet.
            first_check(state)
        }
        Input::AutoCheckToggled(enabled) => vec![Effect::SaveAutoCheck(enabled)],
        Input::BetaOptinToggled(enabled) => {
            // A second press while the first is still settling would resolve
            // in arrival order rather than press order.
            if state.beta_pending {
                return Vec::new();
            }
            // The switch moves at once and locks; Check now reads as busy for
            // the same window, since it is the same check.
            state.beta_optin = Some(enabled);
            state.beta_pending = true;
            state.checking = true;
            state.notice = None;
            vec![Effect::SaveBetaOptin(enabled)]
        }
        Input::BetaOptinApplied { enabled, outcome } => {
            // Unlock before applying the status: it re-syncs the switch from
            // the daemon, and it defers to a pending write.
            state.beta_pending = false;
            match outcome {
                BetaOptinOutcome::Applied(status) => state.apply_status_loaded(status),
                // Nothing changed daemon-side, so the switch goes back.
                BetaOptinOutcome::WriteFailed(e) => {
                    log::warn!("Beta opt-in save failed: {e}");
                    state.beta_optin = Some(!enabled);
                    state.checking = false;
                    state.apply_setting_error(&e);
                }
                BetaOptinOutcome::CheckFailed(e) => {
                    log::warn!("Beta opt-in re-check failed: {e}");
                    state.apply_status_error(&e);
                }
            }
            Vec::new()
        }
        Input::Available => vec![Effect::ReadStatus],
        Input::StartUpdate => start(state),
        Input::CancelUpdate => {
            if state.run.as_ref().is_some_and(UpdateRun::cancellable) {
                // Dropping the stream kills the installer it started.
                if let Some(handle) = state.run_abort.take() {
                    handle.abort();
                }
                state.run = None;
            }
            Vec::new()
        }
        Input::DismissRun => {
            state.dismiss_run();
            Vec::new()
        }
        Input::RunEvent(event) => match state.apply_run_event(event) {
            RunOutcome::Continue => Vec::new(),
            // The installer restarted the daemon: read the status again.
            RunOutcome::RefetchStatus => vec![Effect::ReadStatus],
        },
    }
}

/// A daemon that has not checked yet has no candidate to report, so the page
/// and the header badge read empty until its deferred first check runs: ask
/// once.
fn first_check(state: &mut UpdateState) -> Vec<Effect> {
    if state.wants_first_check() {
        return update(state, Input::CheckNow);
    }
    Vec::new()
}

/// The run, when one may start: an installable candidate and no run going.
fn start(state: &mut UpdateState) -> Vec<Effect> {
    if !state.can_start_update() {
        return Vec::new();
    }
    let Some((asset, tag)) = state.status.as_ref().and_then(|status| {
        Some((
            status.installer_asset.clone()?,
            status.latest_version.clone()?,
        ))
    }) else {
        return Vec::new();
    };
    state.begin_run();
    vec![Effect::StartRun { asset, tag }]
}

/// The script that installs Super Voice from a terminal.
const INSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/super-libre/super-voice/main/install.sh";

/// The terminal command that installs `tag`, with the `--beta` a prerelease
/// needs: offered when there is no installer to run, or the run failed.
#[must_use]
pub fn install_command(tag: &str) -> String {
    let beta_flag = if is_prerelease(tag) {
        " -s -- --beta"
    } else {
        ""
    };
    format!("curl -sSL {INSTALL_SH_URL} | bash{beta_flag}")
}

/// A tag is a prerelease when it carries a semver `-<identifier>` suffix
/// (`v0.2.3-beta.1`), the rule the updater passes `--beta` by.
#[must_use]
pub fn is_prerelease(tag: &str) -> bool {
    tag.contains('-')
}

pub async fn run_status() -> Input {
    match client::get_update_status().await {
        Ok(status) => Input::StatusLoaded(status),
        Err(e) => Input::StatusError(e.to_string()),
    }
}

pub async fn run_check() -> Input {
    match client::check_update_now().await {
        Ok(status) => Input::StatusLoaded(status),
        Err(e) => Input::StatusError(e.to_string()),
    }
}

/// Read whether the daemon checks on its own. A daemon that can't say reads
/// as checking, its default.
pub async fn run_load_auto_check() -> Input {
    Input::AutoCheckLoaded(
        update_check_enabled::get_update_check_enabled()
            .await
            .unwrap_or_else(|e| {
                log::warn!("Failed to load the automatic update check: {e}");
                true
            }),
    )
}

pub async fn run_save_auto_check(enabled: bool) -> Input {
    match update_check_enabled::set_update_check_enabled(enabled).await {
        Ok(()) => Input::AutoCheckLoaded(enabled),
        Err(e) => Input::SettingError(e.to_string()),
    }
}

/// Store the beta switch and, when that worked, check right away: the
/// channel changed, so the candidate is read again.
pub async fn run_save_beta_optin(enabled: bool) -> Input {
    let value = if enabled { "enabled" } else { "disabled" }.to_string();
    let outcome = match update_beta_optin::set_update_beta_optin(value).await {
        Err(e) => BetaOptinOutcome::WriteFailed(e.to_string()),
        Ok(()) => match client::check_update_now().await {
            Ok(status) => BetaOptinOutcome::Applied(status),
            Err(e) => BetaOptinOutcome::CheckFailed(e.to_string()),
        },
    };
    Input::BetaOptinApplied { enabled, outcome }
}

#[cfg(test)]
mod tests {
    use super::{Effect, Input, update};
    use crate::state::update::UpdateState;

    /// A check already running isn't asked for twice.
    #[test]
    fn check_now_asks_once() {
        let mut state = UpdateState::default();
        assert!(matches!(
            update(&mut state, Input::CheckNow)[..],
            [Effect::Check]
        ));
        assert!(update(&mut state, Input::CheckNow).is_empty());
    }

    /// The beta switch moves at once and locks until its write settles; a
    /// failed write puts it back.
    #[test]
    fn the_beta_switch_moves_locks_and_snaps_back_on_a_failed_write() {
        let mut state = UpdateState::default();
        assert!(matches!(
            update(&mut state, Input::BetaOptinToggled(true))[..],
            [Effect::SaveBetaOptin(true)]
        ));
        assert_eq!(state.beta_optin, Some(true));
        assert!(
            update(&mut state, Input::BetaOptinToggled(false)).is_empty(),
            "locked"
        );
        update(
            &mut state,
            Input::BetaOptinApplied {
                enabled: true,
                outcome: super::BetaOptinOutcome::WriteFailed("down".into()),
            },
        );
        assert_eq!(state.beta_optin, Some(false));
        assert!(!state.beta_pending);
        assert!(state.notice.is_some());
    }

    #[test]
    fn a_beta_installs_with_the_beta_flag() {
        assert!(super::install_command("v0.3.0-beta.7").ends_with("| bash -s -- --beta"));
        assert!(super::install_command("v0.3.0").ends_with("| bash"));
    }

    /// No candidate, no run.
    #[test]
    fn an_update_starts_only_with_a_candidate() {
        let mut state = UpdateState::default();
        assert!(update(&mut state, Input::StartUpdate).is_empty());
    }
}
