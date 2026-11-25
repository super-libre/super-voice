// SPDX-License-Identifier: GPL-3.0-only
//! What the app does as the executable of `Super Voice.app`: post the
//! daemon's banners ([`notifier`]), and, when it opens, register the bundle's
//! `LaunchAgent`s and ask to show notifications.
//!
//! None of it applies to a bare binary in `target/` (`just run-app`). That
//! has no bundle for `ServiceManagement` to find the agents in, and none for
//! Notification Center to attribute banners to, which raises an exception
//! there rather than returning an error.

pub mod notifier;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSBundle, NSError};
use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};

/// Whether this process is the executable of an app bundle.
#[must_use]
pub fn in_bundle() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some()
        && std::path::Path::new(&bundle.bundlePath().to_string())
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

/// What the app does once it opens from its bundle: register each agent that
/// has never been registered, which starts the daemon and the shortcut and
/// runs them at every login, and ask to show notifications.
///
/// Registering is what installs the daemon for someone who drags Super Voice
/// into Applications and opens it, so it runs on every launch rather than
/// once. It is blocking (`ServiceManagement` answers over XPC), so it runs on
/// a thread of its own.
///
/// The daemon's failures are posted by this executable in notify mode, so
/// the permission to show them is this bundle's. Asked from here, the prompt
/// comes up while the user is looking at Super Voice rather than the first
/// time a dictation fails. Once the user has answered, macOS answers for
/// them.
pub fn on_launch() {
    if !in_bundle() {
        return;
    }
    std::thread::spawn(super_voice_app_core::pages::connection::register_agents);

    let block = RcBlock::new(|granted: Bool, error: *mut NSError| {
        // SAFETY: the framework passes either null or an `NSError` that is
        // live for the length of the call.
        match unsafe { error.as_ref() } {
            Some(e) => log::warn!(
                "could not ask to show notifications: {}",
                e.localizedDescription()
            ),
            None => log::debug!("notifications allowed: {}", granted.as_bool()),
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &block);
}
