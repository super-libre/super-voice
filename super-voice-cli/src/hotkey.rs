// SPDX-License-Identifier: GPL-3.0-only
//! `voice hotkey` — a global keyboard shortcut that starts and stops a
//! recording, for macOS.
//!
//! On Linux the desktop environment owns global shortcuts (Wayland gives no
//! client a system-wide key grab), and it is the DE that runs
//! `voice record --write` when the key is pressed. macOS has no equivalent place
//! to bind a key to a command, so this subcommand stands in for the DE: a
//! long-running client that owns one shortcut and, on each press, does exactly
//! what `voice record --write` does, through the same function. The daemon never
//! learns anything about input.
//!
//! Registration goes through Carbon's `RegisterEventHotKey`, by way of the
//! `global-hotkey` crate. That API needs no Accessibility or Input Monitoring
//! grant, which matters here more than usual: a TCC grant is one more thing
//! that silently stops matching when a binary changes. The crate switches to a
//! `CGEventTap` — which does need Input Monitoring — only for media keys, so a
//! binding naming one of those will want that permission (the daemon refuses
//! one as the `hotkey` setting for that reason).
//!
//! The keys are the daemon's `hotkey` setting, read at start and again
//! whenever `settings_changed` names it, so the app's Shortcuts page can
//! change them: the listener swaps the shortcut it holds at once, on the main
//! thread. `--key` is used only until the daemon first answers.

use anyhow::{Context, Result, anyhow};
use clap::{Arg, ArgAction, Command};
use futures_util::StreamExt;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use super_voice_shared::daemon::http_client;
use super_voice_shared::daemon::session::{self, AppId};
use super_voice_shared::daemon::widget_subscription::{
    WidgetSubscriptionConfig, WidgetSubscriptionUpdate, run_widget_subscription,
};
use super_voice_shared::models::hotkey as binding;

use crate::{HOTKEY_SCOPES, HOTKEY_TOPICS};

/// The listener's own identity to the daemon, apart from `stt`'s.
///
/// In the macOS app bundle the two are copies of one binary at different
/// paths: `stt` in `Contents/MacOS`, and the listener in `Contents/Helpers`,
/// since beside the settings app its event loop would register with macOS
/// as the app itself. The daemon binds each session to the path that
/// obtained it, so sharing `stt`'s keychain entry would have each revoke the
/// other's session every time it was used.
const APP_ID: AppId = session::app_id("super-voice-hotkey");
const APP_NAME: &str = "Super Voice Shortcut";

/// ⌃⌥Space, until the daemon says otherwise. See
/// [`binding::DEFAULT_BINDING`].
pub const DEFAULT_BINDING: &str = binding::DEFAULT_BINDING;

/// How long the listener waits for the daemon's keys at start before it
/// listens for `--key`. At login the daemon may not be up yet; the keys it
/// has are picked up as soon as it is.
const FIRST_READ: Duration = Duration::from_secs(3);

pub fn command() -> Command {
    Command::new("hotkey")
        .about("Listen for a global shortcut that starts and stops recording (macOS)")
        .long_about(
            "Listen for a global shortcut that starts and stops recording.\n\n\
             Runs until stopped. Each press does what `voice record --write` does: \
             starts a recording, or stops the one in progress and types the result \
             into the focused window. The Super Voice app runs this as a \
             LaunchAgent so it is available from login.\n\n\
             The keys are the daemon's `hotkey` setting, which the app's Shortcuts \
             page changes; the listener follows a change at once. `--key` is used \
             only until the daemon answers.",
        )
        .arg(
            Arg::new("key")
                .long("key")
                .value_name("BINDING")
                .default_value(DEFAULT_BINDING)
                .help(
                    "Shortcut to listen for until the daemon answers, as modifiers and a \
                     key joined by `+`: ctrl, alt/option, cmd/super, shift, then a key \
                     such as space or KeyR",
                ),
        )
        .arg(
            Arg::new("check")
                .long("check")
                .help("Only validate the binding, then exit")
                .action(ArgAction::SetTrue),
        )
}

/// Parse a binding, with an error that says what the accepted form is.
fn parse_binding(binding: &str) -> Result<HotKey> {
    binding.parse().map_err(|e| {
        anyhow!(
            "`{binding}` is not a shortcut ({e}). \
             Use modifiers and one key joined by `+`, e.g. `{DEFAULT_BINDING}` or `cmd+shift+KeyR`."
        )
    })
}

/// `--check`: validate without registering anything.
pub fn check(binding: &str) -> Result<()> {
    parse_binding(binding).map(|_| ())
}

/// The shortcut the listener holds, shared by the thread that serves presses
/// and the task that follows the daemon.
struct Listener {
    /// Held for the life of the process: dropping the manager unregisters
    /// the shortcut. Its calls go through the main thread (see
    /// [`Listener::swap`]).
    manager: GlobalHotKeyManager,
    /// The shortcut registered now. `None` while the keys asked for could not
    /// be registered and no earlier ones were.
    current: Mutex<Option<HotKey>>,
}

impl Listener {
    fn current(&self) -> MutexGuard<'_, Option<HotKey>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Listen for `next` in place of the shortcut held now. Must run on the
    /// main thread: `global-hotkey`'s macOS backend registers with Carbon,
    /// whose hotkeys belong to the thread running the application's event
    /// loop, which here is the main one.
    ///
    /// The new shortcut is registered before the old one is let go of, so
    /// keys another app holds leave the listener on the keys it had, with a
    /// warning in its log, rather than on none.
    fn swap_on_main(&self, _on_main: MainThreadMarker, next: HotKey) {
        let mut current = self.current();
        if *current == Some(next) {
            return;
        }
        if let Err(e) = self.manager.register(next) {
            match *current {
                Some(held) => log::warn!(
                    "could not listen for {next} ({e}); another app may be using it. \
                     Still listening for {held}"
                ),
                None => log::warn!(
                    "could not listen for {next} ({e}); another app may be using it. \
                     Listening for nothing until the keys change"
                ),
            }
            return;
        }
        if let Some(old) = current.replace(next)
            && let Err(e) = self.manager.unregister(old)
        {
            log::warn!("could not stop listening for {old}: {e}");
        }
        log::info!("Listening for {next}; press it to start or stop a recording");
    }

    /// [`Self::swap_on_main`], from any thread. Blocks until the main thread
    /// has done it.
    fn swap(self: &Arc<Self>, next: HotKey) {
        let listener = Arc::clone(self);
        dispatch2::run_on_main(move |on_main| listener.swap_on_main(on_main, next));
    }
}

/// The keys the daemon has for the listener.
async fn read_binding(socket_path: &Path) -> Result<String> {
    let socket_path = socket_path.to_path_buf();
    crate::run_with_token_as(
        APP_ID,
        APP_NAME,
        HOTKEY_SCOPES,
        socket_path.clone(),
        move |token| {
            let socket_path = socket_path.clone();
            async move { http_client::hotkey(socket_path, &token).await }
        },
    )
    .await
}

/// Read the daemon's keys and listen for them.
async fn follow_binding(socket_path: &Path, listener: &Arc<Listener>) {
    let next = match read_binding(socket_path).await {
        Ok(text) => match parse_binding(&text) {
            Ok(next) => next,
            Err(e) => {
                log::warn!("the daemon's shortcut cannot be used: {e:#}");
                return;
            }
        },
        Err(e) => {
            log::warn!("could not read the shortcut's keys: {e:#}");
            return;
        }
    };
    let listener = Arc::clone(listener);
    // `swap` waits on the main thread; not on a runtime worker.
    if let Err(e) = tokio::task::spawn_blocking(move || listener.swap(next)).await {
        log::warn!("could not change the shortcut: {e}");
    }
}

/// Follow the daemon's events: read the keys again whenever they change, and
/// whenever the daemon is back, since a change made while it could not be
/// heard would otherwise be missed. Runs for the life of the process, unless
/// the user refuses the listener.
async fn follow(socket_path: &Path, listener: &Arc<Listener>) {
    let config = WidgetSubscriptionConfig::new(APP_ID, APP_NAME, HOTKEY_SCOPES, HOTKEY_TOPICS);
    let mut updates = Box::pin(run_widget_subscription(socket_path.to_path_buf(), config));
    while let Some(update) = updates.next().await {
        let read_again = match update {
            WidgetSubscriptionUpdate::Connected => true,
            WidgetSubscriptionUpdate::Event(event) => {
                binding::changed_in(&event.name, &event.payload)
            }
            WidgetSubscriptionUpdate::Blocked { reason } => {
                log::warn!(
                    "the daemon refused the shortcut listener ({reason}); \
                     its keys no longer follow the app's setting"
                );
                false
            }
            WidgetSubscriptionUpdate::Disconnected { .. }
            | WidgetSubscriptionUpdate::NeedsReauth { .. } => false,
        };
        if read_again {
            follow_binding(socket_path, listener).await;
        }
    }
}

/// Listen for the daemon's keys, or `fallback` until it answers, and serve
/// presses until the process is stopped.
///
/// Must be called on the main thread before any tokio runtime exists: the
/// crate's macOS backend needs an event loop running on the main thread, and
/// that loop — `NSApplication::run` — never returns. The runtime that talks to
/// the daemon lives on worker threads instead.
pub fn run(socket_path: PathBuf, fallback: &str) -> Result<()> {
    let on_main = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("`voice hotkey` must run on the process's main thread"))?;
    let fallback = parse_binding(fallback)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not start the async runtime")?;
    let handle = runtime.handle().clone();

    // The timeout is made inside the runtime: its timer needs one to exist.
    let first = match runtime
        .block_on(async { tokio::time::timeout(FIRST_READ, read_binding(&socket_path)).await })
    {
        Ok(Ok(text)) => parse_binding(&text)
            .inspect_err(|e| log::warn!("the daemon's shortcut cannot be used: {e:#}"))
            .unwrap_or(fallback),
        Ok(Err(e)) => {
            log::info!(
                "the daemon has not answered yet ({e:#}); listening for {fallback} until it does"
            );
            fallback
        }
        Err(_) => {
            log::info!("the daemon has not answered yet; listening for {fallback} until it does");
            fallback
        }
    };

    let listener = Arc::new(Listener {
        manager: GlobalHotKeyManager::new().context("could not start the hotkey manager")?,
        current: Mutex::new(None),
    });
    listener.swap_on_main(on_main, first);
    let (socket, follower) = (socket_path.clone(), Arc::clone(&listener));
    runtime.spawn(async move { follow(&socket, &follower).await });

    // Presses are handled one at a time, in order, on this thread. The
    // start-or-stop decision in `cmd_record` comes from the daemon's `busy`
    // flag, so two presses in flight together would both read "not busy" and
    // both try to start; serialized, the second press sees the recording the
    // first one began and stops it.
    let dispatcher = Arc::clone(&listener);
    std::thread::Builder::new()
        .name("hotkey-dispatch".into())
        .spawn(move || {
            while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
                if event.state != HotKeyState::Pressed {
                    continue;
                }
                let Some(pressed) = *dispatcher.current() else {
                    continue;
                };
                if event.id != pressed.id() {
                    continue;
                }
                log::debug!("{pressed} pressed");
                let result = handle.block_on(crate::run_with_token_as(
                    APP_ID,
                    APP_NAME,
                    HOTKEY_SCOPES,
                    socket_path.clone(),
                    // A shortcut's take brings no context of its own.
                    |token| crate::cmd_record(socket_path.clone(), token, true, false, None, None),
                ));
                if let Err(e) = result {
                    // Logged, not fatal: the daemon being down or restarting is
                    // no reason to stop listening for the next press.
                    log::warn!("{pressed}: {e:#}");
                }
            }
        })
        .context("could not start the hotkey dispatch thread")?;

    let app = NSApplication::sharedApplication(on_main);
    // No Dock icon, no menu bar, never frontmost. This process has no UI; the
    // application object exists only to run the event loop the shortcut is
    // delivered on, and the main queue the swaps are sent to.
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);

    app.run();

    // `run` returns only if something sends NSApp `stop:`, which nothing here
    // does. Named so the order they are released in is not left to chance.
    drop(listener);
    drop(runtime);
    Ok(())
}
