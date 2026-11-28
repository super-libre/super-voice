// SPDX-License-Identifier: GPL-3.0-only
//! What the app does as the executable of `Super Voice.app`: post the
//! daemon's banners ([`notifier`]), when it opens, register the bundle's
//! `LaunchAgent`s and ask to show notifications, and ask macOS to let it
//! type into other apps for the daemon ([`ask_to_type`]).
//!
//! None of it applies to a bare binary in `target/` (`just run-app`). That
//! has no bundle for `ServiceManagement` to find the agents in, and none for
//! Notification Center to attribute banners to, which raises an exception
//! there rather than returning an error.

pub mod notifier;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSBundle, NSError};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNUserNotificationCenter,
};

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

/// Whether macOS shows this bundle's notifications, which the daemon's
/// banners are (see [`notifier`]): `None` outside a bundle, where
/// Notification Center can't be asked, or when it doesn't answer. Blocking:
/// it waits for Notification Center, so it runs off the UI thread.
#[must_use]
pub fn notifications_allowed() -> Option<bool> {
    if !in_bundle() {
        return None;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    match notifier::authorization_status(&center) {
        Ok(status) => Some(
            status == UNAuthorizationStatus::Authorized
                || status == UNAuthorizationStatus::Provisional,
        ),
        Err(e) => {
            log::warn!("{e:#}");
            None
        }
    }
}

/// Ask macOS whether this app may control the computer (Device Control,
/// called Accessibility before macOS 27), and let it put up its own prompt
/// if not. Asking is what adds Super Voice to the pane's list, switched off.
///
/// The app asks for the daemon, which can't: macOS lets no background agent
/// prompt, and it decides for the daemon by the app the daemon runs inside,
/// so the app's switch is the one that lets the daemon type. Blocking: it
/// waits for macOS, so it runs off the UI thread.
///
/// Not from a bare binary, which macOS would answer for the terminal it was
/// started from.
pub fn ask_to_type() {
    use std::ffi::c_void;

    /// A `CFTypeRef`: any Core Foundation object.
    type CfType = *const c_void;

    /// A Core Foundation callbacks table, only ever passed by address.
    #[repr(C)]
    struct Callbacks {
        _private: [u8; 0],
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: CfType;
        static kCFTypeDictionaryKeyCallBacks: Callbacks;
        static kCFTypeDictionaryValueCallBacks: Callbacks;
        fn CFDictionaryCreate(
            allocator: CfType,
            keys: *const CfType,
            values: *const CfType,
            count: isize,
            key_callbacks: *const Callbacks,
            value_callbacks: *const Callbacks,
        ) -> CfType;
        fn CFRelease(object: CfType);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        static kAXTrustedCheckOptionPrompt: CfType;
        /// `Boolean AXIsProcessTrustedWithOptions(CFDictionaryRef)`.
        fn AXIsProcessTrustedWithOptions(options: CfType) -> u8;
    }

    if !in_bundle() {
        return;
    }
    // SAFETY: the two statics are constant objects the frameworks define,
    // and the dictionary retains its key and value with the standard
    // callbacks. It is released once, after the one call that reads it.
    let trusted = unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        );
        if options.is_null() {
            log::warn!("could not ask macOS whether Super Voice may type");
            return;
        }
        let trusted = AXIsProcessTrustedWithOptions(options) != 0;
        CFRelease(options);
        trusted
    };
    log::debug!("asked macOS whether Super Voice may type: {trusted}");
}
