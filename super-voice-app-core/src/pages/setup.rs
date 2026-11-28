// SPDX-License-Identifier: GPL-3.0-only
//! What macOS has to allow before Super Voice works in full, and how the app
//! leads the user there.
//!
//! Two permissions matter. Typing into other apps is required: the daemon
//! needs Device Control (called Accessibility before macOS 27), and without
//! it a dictation goes to the clipboard instead. Notifications are optional:
//! without them a failure shows only in the app.
//!
//! The app says so in four places, all from this module:
//!
//! 1. The Typed step on Speech to text › Models turns into a card that says
//!    what is wrong and how to fix it ([`typed_card`]), and folds back by
//!    itself once typing is allowed.
//! 2. A Finish setup page, first in the sidebar while typing is not allowed
//!    ([`setup_row_shown`]). The app opens on it at first launch if typing is
//!    not allowed ([`opens_on_setup`]).
//! 3. Settings › Notifications says whether macOS shows Super Voice's
//!    notifications.
//! 4. A dictation that could not be typed shows a toast ([`toast`]).
//!
//! Whether the daemon may type is the daemon's to say (`GET /status`'s
//! `stt.can_type`), read into the window's readiness
//! ([`crate::pages::readiness::typing_blocked`]); while the card or the page
//! is on screen, the app asks again every [`CHECK_EVERY`].
//! Asking macOS is the app's ([`Effect::AskToType`]): macOS decides for the
//! daemon by the app it runs inside, and lets no background agent prompt.
//! Whether notifications are on is the app's own bundle's to say, since the
//! daemon's banners are posted as the app.

use std::path::Path;
use std::time::Duration;

use super_voice_shared::models::notification_method::NotificationMethod;

use crate::events::{TypingFailed, TypingFailure};

/// How often the app asks the daemon whether it may type, while it shows how
/// to let it.
pub const CHECK_EVERY: Duration = Duration::from_secs(2);

/// How long the Typed card shows Ready before it folds back into the plain
/// step.
pub const READY_FOR: Duration = Duration::from_secs(5);

/// How long a Copy button says Copied.
pub const COPIED_FOR: Duration = Duration::from_secs(2);

/// System Settings at the pane that holds the switch. The same link opens
/// Device Control on macOS 27 and Accessibility before it.
pub const TYPING_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/// The app bundle's identifier, whose notifications System Settings shows.
pub const BUNDLE_ID: &str = "org.superlibre.super-voice-app";

/// System Settings at Notifications, on Super Voice's own entry.
#[must_use]
pub fn notifications_url() -> String {
    format!("x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={BUNDLE_ID}")
}

/// The daemon's file name, beside the app in `Contents/MacOS`.
pub const DAEMON_FILE: &str = "super-voice-daemon";

/// The daemon beside the app `exe`: in the bundle, both are in
/// `Contents/MacOS`. What the user adds by its path when the daemon isn't in
/// the pane's list.
#[must_use]
pub fn daemon_beside(exe: &Path) -> String {
    exe.parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(DAEMON_FILE)
        .display()
        .to_string()
}

/// The pane in Privacy & Security that holds the switch, by what macOS calls
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    /// macOS 27 and later.
    #[default]
    DeviceControl,
    /// Before macOS 27.
    Accessibility,
}

impl Pane {
    /// The pane on macOS `major`. Not knowing is the newest.
    #[must_use]
    pub const fn for_macos(major: Option<u64>) -> Self {
        match major {
            Some(major) if major < 27 => Self::Accessibility,
            _ => Self::DeviceControl,
        }
    }

    /// The pane on the Mac whose kernel is Darwin `release` ("26.0.0").
    ///
    /// The kernel's version, not the one macOS reports to apps: macOS hands
    /// an app built against an older SDK a made-up version ("16.0" on macOS
    /// 26), and the kernel's is never made up. Darwin 20 to 24 are macOS 11
    /// to 15; from Darwin 25, macOS 26, the two are one apart.
    #[must_use]
    pub fn for_darwin(release: &str) -> Self {
        let darwin = release
            .split('.')
            .next()
            .and_then(|major| major.parse::<u64>().ok());
        Self::for_macos(darwin.map(|darwin| match darwin {
            ..=24 => darwin.saturating_sub(9),
            _ => darwin + 1,
        }))
    }

    /// The pane on this Mac; elsewhere, the newest name.
    #[must_use]
    pub fn here() -> Self {
        #[cfg(target_os = "macos")]
        {
            darwin_release().map_or_else(Self::default, |release| Self::for_darwin(&release))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::default()
        }
    }

    /// Its name, as a message key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::DeviceControl => "typing-pane-device-control",
            Self::Accessibility => "typing-pane-accessibility",
        }
    }
}

/// The kernel's release, from `uname`: "26.0.0".
#[cfg(target_os = "macos")]
fn darwin_release() -> Option<String> {
    // SAFETY: `utsname` is plain bytes, and `uname` fills it or fails.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is a valid, writable `utsname`.
    if unsafe { libc::uname(&raw mut name) } != 0 {
        return None;
    }
    // SAFETY: `uname` NUL-terminates `release`.
    let release = unsafe { std::ffi::CStr::from_ptr(name.release.as_ptr()) };
    Some(release.to_string_lossy().into_owned())
}

/// What the Typed step on Speech to text › Models shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedCard {
    /// The plain step: typing works, or the daemon can't tell.
    Plain,
    /// A Needs setup card with the two steps.
    NeedsSetup,
    /// The same, after Open Device Control: the app is checking.
    Checking,
    /// Typing was just allowed: Ready, for [`READY_FOR`], then
    /// [`Self::Plain`].
    Ready,
}

/// Where typing stands, for the Typed card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Typing {
    /// Allowed, or the daemon can't tell.
    #[default]
    Allowed,
    /// Not allowed.
    Blocked,
    /// Not allowed, and Open Device Control was pressed: checking.
    Checking,
    /// Just allowed: the card says Ready.
    JustAllowed,
}

impl Typing {
    const fn blocked(self) -> bool {
        matches!(self, Self::Blocked | Self::Checking)
    }
}

/// What the app keeps about the setup.
#[derive(Debug, Clone, Default)]
pub struct SetupState {
    /// The pane's name on this Mac.
    pub pane: Pane,
    /// Where the daemon is, to add it by its path.
    pub daemon_path: String,
    /// Whether macOS shows Super Voice's notifications: `None` where the app
    /// can't tell (not macOS, or not run from its bundle).
    pub notifications: Option<bool>,
    /// How failures are reported, as the daemon last said: `None` until read.
    pub notification_method: Option<NotificationMethod>,
    /// Where typing stands, as last heard: what a change is measured
    /// against.
    typing: Typing,
    /// The app asked macOS for the permission since it opened.
    asked: bool,
    /// The path was just copied.
    pub copied: bool,
    /// The Finish setup page was opened on by itself.
    opened_on_setup: bool,
}

impl SetupState {
    /// The setup of a Mac whose pane is `pane`, with the daemon at
    /// `daemon_path`; nothing else known yet.
    #[must_use]
    pub fn new(pane: Pane, daemon_path: String) -> Self {
        Self {
            pane,
            daemon_path,
            ..Self::default()
        }
    }
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The daemon answers: read what the app shows of the setup.
    Reached,
    /// Whether the daemon may type, as the readiness says now: `None` while
    /// it isn't known (the daemon can't tell, can't be reached, or hasn't
    /// said yet), which changes nothing.
    Typing { can_type: Option<bool> },
    /// Open Device Control (Accessibility) was pressed.
    OpenPane,
    /// Open Notifications was pressed.
    OpenNotifications,
    /// The app's window came to the front: the user may be back from System
    /// Settings, so read again whether macOS shows notifications.
    WindowActive,
    /// Copy was pressed beside the daemon's path.
    CopyPath,
    /// The Copied mark has shown long enough.
    CopiedFaded,
    /// The Ready card has shown long enough.
    Folded,
    /// Whether macOS shows Super Voice's notifications.
    NotificationsRead(Option<bool>),
    /// How failures are reported.
    MethodRead(NotificationMethod),
    /// Another client changed a setting, by the name the daemon gives it.
    SettingChanged(String),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Ask macOS whether the app may control the computer, letting it show
    /// its own prompt if not. That puts Super Voice in the pane's list,
    /// switched off, and the app's switch is the one that counts for the
    /// daemon: macOS decides for the daemon by the app it is in. The daemon
    /// can't ask itself, since macOS lets no background agent prompt.
    /// An [`Effect::Open`] that follows opens the pane once macOS was asked.
    AskToType,
    /// Open this link: a System Settings pane.
    Open(String),
    /// Put this on the clipboard.
    Copy(String),
    /// Send [`Input::CopiedFaded`] after [`COPIED_FOR`].
    FadeCopied,
    /// Send [`Input::Folded`] after [`READY_FOR`].
    FoldLater,
    /// Ask macOS whether it shows Super Voice's notifications, and send
    /// [`Input::NotificationsRead`].
    ReadNotifications,
    /// Read how failures are reported ([`run_read_method`]).
    ReadMethod,
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut SetupState, input: Input) -> Vec<Effect> {
    match input {
        Input::Reached => vec![Effect::ReadMethod, Effect::ReadNotifications],
        Input::Typing { can_type } => match (state.typing.blocked(), can_type) {
            (true, Some(true)) => {
                state.typing = Typing::JustAllowed;
                vec![Effect::FoldLater]
            }
            (false, Some(false)) => {
                state.typing = Typing::Blocked;
                if state.asked {
                    return Vec::new();
                }
                state.asked = true;
                vec![Effect::AskToType]
            }
            _ => Vec::new(),
        },
        Input::OpenPane => {
            if state.typing.blocked() {
                state.typing = Typing::Checking;
            }
            vec![Effect::AskToType, Effect::Open(TYPING_PANE_URL.to_string())]
        }
        // Read again when the user is back ([`Input::WindowActive`]).
        Input::OpenNotifications => vec![Effect::Open(notifications_url())],
        Input::WindowActive => vec![Effect::ReadNotifications],
        Input::CopyPath => {
            state.copied = true;
            vec![Effect::Copy(state.daemon_path.clone()), Effect::FadeCopied]
        }
        Input::CopiedFaded => {
            state.copied = false;
            Vec::new()
        }
        Input::Folded => {
            if state.typing == Typing::JustAllowed {
                state.typing = Typing::Allowed;
            }
            Vec::new()
        }
        Input::NotificationsRead(on) => {
            state.notifications = on;
            Vec::new()
        }
        Input::MethodRead(method) => {
            state.notification_method = Some(method);
            Vec::new()
        }
        Input::SettingChanged(name) if name == "notification_method" => vec![Effect::ReadMethod],
        Input::SettingChanged(_) => Vec::new(),
    }
}

/// What the Typed step shows.
#[must_use]
pub fn typed_card(state: &SetupState) -> TypedCard {
    match state.typing {
        Typing::Allowed => TypedCard::Plain,
        Typing::Blocked => TypedCard::NeedsSetup,
        Typing::Checking => TypedCard::Checking,
        Typing::JustAllowed => TypedCard::Ready,
    }
}

/// Whether the Finish setup row is in the sidebar: while typing is not
/// allowed, and while its page is shown, so that it doesn't vanish under the
/// user the moment typing is allowed. Notifications alone never keep it.
#[must_use]
pub const fn setup_row_shown(typing_blocked: bool, setup_shown: bool) -> bool {
    typing_blocked || setup_shown
}

/// Whether the app asks the daemon every [`CHECK_EVERY`] whether it may
/// type: while it is not allowed and the fix is on screen (the Typed card,
/// or the Finish setup page).
#[must_use]
pub const fn checks_typing(typing_blocked: bool, fix_shown: bool) -> bool {
    typing_blocked && fix_shown
}

/// Whether the app goes to Finish setup now: once, at the first launch of
/// the app, as soon as the daemon says it may not type.
#[must_use]
pub fn opens_on_setup(state: &mut SetupState, first_launch: bool, typing_blocked: bool) -> bool {
    if first_launch && typing_blocked && !state.opened_on_setup {
        state.opened_on_setup = true;
        return true;
    }
    false
}

/// Whether this is the app's first launch on this account, from a mark it
/// leaves in `dir`: absent the first time, and left then for every launch
/// after. A mark that can't be written makes every launch the first, which
/// costs no more than the page opening while typing is off.
#[must_use]
pub fn first_launch(dir: &Path) -> bool {
    let mark = dir.join("app-opened");
    if mark.exists() {
        return false;
    }
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&mark, b"")) {
        log::warn!(
            "could not leave the first-launch mark in {}: {e}",
            dir.display()
        );
    }
    true
}

/// The toast for a dictation that could not be typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingToast {
    /// Its words are on the clipboard.
    pub copied: bool,
    /// The toast offers Allow typing, which goes to the Typed card: only when
    /// the permission is what stopped it.
    pub allow_typing: bool,
    /// The words' first line, quoted.
    pub quote: String,
}

/// The toast a dictation that could not be typed shows, if any.
///
/// The app shows it whether or not macOS shows the daemon's banner: it is
/// the one that says how to let the daemon type, and it is the fallback when
/// Failure notices are desktop notifications and macOS has them off. Only
/// Failure notices switched off (log only) leave it out.
#[must_use]
pub fn toast(failed: &TypingFailed, method: Option<NotificationMethod>) -> Option<TypingToast> {
    if method == Some(NotificationMethod::Off) {
        return None;
    }
    Some(TypingToast {
        copied: failed.copied,
        allow_typing: failed.reason == TypingFailure::NotAllowed,
        quote: quote(&failed.text),
    })
}

/// The words' first line, in quotation marks.
fn quote(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    line.map_or_else(String::new, |line| format!("“{line}”"))
}

/// Read how failures are reported.
pub async fn run_read_method() -> Option<Input> {
    crate::daemon::client::get_notification_method()
        .await
        .map_err(|e| log::debug!("couldn't read how failures are reported: {e}"))
        .ok()
        .map(|name| Input::MethodRead(name.parse().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(reason: TypingFailure, copied: bool) -> TypingFailed {
        TypingFailed {
            reason,
            copied,
            text: "Call the landlord about the heater before Friday.\nAnd the gas.".to_string(),
        }
    }

    /// The pane is Device Control on macOS 27 and later, Accessibility before;
    /// not knowing is the newest.
    #[test]
    fn the_pane_is_named_as_this_macos_names_it() {
        assert_eq!(Pane::for_macos(Some(26)), Pane::Accessibility);
        assert_eq!(Pane::for_macos(Some(15)), Pane::Accessibility);
        assert_eq!(Pane::for_macos(Some(27)), Pane::DeviceControl);
        assert_eq!(Pane::for_macos(Some(28)), Pane::DeviceControl);
        assert_eq!(Pane::for_macos(None), Pane::DeviceControl);
        // From the kernel: Darwin 24 is macOS 15, 25 is 26, 26 is 27.
        assert_eq!(Pane::for_darwin("24.6.0"), Pane::Accessibility);
        assert_eq!(Pane::for_darwin("25.1.0"), Pane::Accessibility);
        assert_eq!(Pane::for_darwin("26.0.0"), Pane::DeviceControl);
        assert_eq!(Pane::for_darwin("not a version"), Pane::DeviceControl);
    }

    #[test]
    fn the_daemon_is_beside_the_app() {
        assert_eq!(
            daemon_beside(Path::new(
                "/Applications/Super Voice.app/Contents/MacOS/super-voice-app"
            )),
            "/Applications/Super Voice.app/Contents/MacOS/super-voice-daemon"
        );
    }

    /// The card's life: needs setup, checking after Open Device Control,
    /// Ready once allowed, then the plain step.
    #[test]
    fn the_typed_card_needs_setup_checks_then_folds_back() {
        let mut state = SetupState::default();
        assert_eq!(typed_card(&state), TypedCard::Plain);

        update(
            &mut state,
            Input::Typing {
                can_type: Some(false),
            },
        );
        assert_eq!(typed_card(&state), TypedCard::NeedsSetup);

        assert_eq!(
            update(&mut state, Input::OpenPane),
            [Effect::AskToType, Effect::Open(TYPING_PANE_URL.to_string())]
        );
        assert_eq!(typed_card(&state), TypedCard::Checking);

        // Still blocked at the next check: nothing changes.
        assert!(
            update(
                &mut state,
                Input::Typing {
                    can_type: Some(false)
                }
            )
            .is_empty()
        );
        assert_eq!(typed_card(&state), TypedCard::Checking);

        assert_eq!(
            update(
                &mut state,
                Input::Typing {
                    can_type: Some(true)
                }
            ),
            [Effect::FoldLater]
        );
        assert_eq!(typed_card(&state), TypedCard::Ready);

        update(&mut state, Input::Folded);
        assert_eq!(typed_card(&state), TypedCard::Plain);
    }

    /// The app asks macOS once after it opens, the first time the daemon
    /// says it may not type, and again at every Open Device Control. Asking
    /// is what puts Super Voice in the pane's list.
    #[test]
    fn the_app_asks_macos_once_it_hears_typing_is_off() {
        let mut state = SetupState::default();
        let blocked = Input::Typing {
            can_type: Some(false),
        };
        assert_eq!(update(&mut state, blocked.clone()), [Effect::AskToType]);
        assert!(update(&mut state, blocked.clone()).is_empty());

        // Allowed, then off again: not asked again until the button.
        update(
            &mut state,
            Input::Typing {
                can_type: Some(true),
            },
        );
        assert!(update(&mut state, blocked).is_empty());
        assert_eq!(
            update(&mut state, Input::OpenPane).first(),
            Some(&Effect::AskToType)
        );
    }

    /// Typing that was never blocked shows no Ready card: an app that opens
    /// while it is allowed has nothing to celebrate.
    #[test]
    fn allowed_from_the_start_is_the_plain_step() {
        let mut state = SetupState::default();
        assert!(
            update(
                &mut state,
                Input::Typing {
                    can_type: Some(true)
                }
            )
            .is_empty()
        );
        assert_eq!(typed_card(&state), TypedCard::Plain);
    }

    /// Not knowing (the daemon down, or not said yet) changes nothing: the
    /// card neither clears nor celebrates.
    #[test]
    fn not_knowing_changes_nothing() {
        let mut state = SetupState::default();
        update(
            &mut state,
            Input::Typing {
                can_type: Some(false),
            },
        );
        assert!(update(&mut state, Input::Typing { can_type: None }).is_empty());
        assert_eq!(typed_card(&state), TypedCard::NeedsSetup);
    }

    /// Switched off again while the Ready card shows: it needs setup again.
    #[test]
    fn blocked_again_needs_setup_again() {
        let mut state = SetupState::default();
        update(
            &mut state,
            Input::Typing {
                can_type: Some(false),
            },
        );
        update(
            &mut state,
            Input::Typing {
                can_type: Some(true),
            },
        );
        update(
            &mut state,
            Input::Typing {
                can_type: Some(false),
            },
        );
        assert_eq!(typed_card(&state), TypedCard::NeedsSetup);
    }

    #[test]
    fn copy_says_copied_for_a_moment() {
        let mut state = SetupState {
            daemon_path: "/Applications/Super Voice.app/Contents/MacOS/super-voice-daemon".into(),
            ..SetupState::default()
        };
        assert_eq!(
            update(&mut state, Input::CopyPath),
            [
                Effect::Copy(
                    "/Applications/Super Voice.app/Contents/MacOS/super-voice-daemon".into()
                ),
                Effect::FadeCopied
            ]
        );
        assert!(state.copied);
        update(&mut state, Input::CopiedFaded);
        assert!(!state.copied);
    }

    /// The row appears while typing is blocked, stays while its page is shown
    /// after typing is allowed, and leaves once another page is. Notifications
    /// off never keep it.
    #[test]
    fn the_finish_setup_row_comes_and_goes() {
        assert!(setup_row_shown(true, false));
        assert!(setup_row_shown(true, true));
        assert!(setup_row_shown(false, true));
        assert!(!setup_row_shown(false, false));

        let mut state = SetupState::default();
        update(&mut state, Input::NotificationsRead(Some(false)));
        assert!(!setup_row_shown(false, false), "{state:?}");
    }

    #[test]
    fn typing_is_checked_only_while_the_fix_is_on_screen() {
        assert!(checks_typing(true, true));
        assert!(!checks_typing(true, false));
        assert!(!checks_typing(false, true));
    }

    /// At first launch, once, and only while typing is not allowed.
    #[test]
    fn the_app_opens_on_finish_setup_once_at_first_launch() {
        let mut state = SetupState::default();
        assert!(!opens_on_setup(&mut state, true, false));
        assert!(!opens_on_setup(&mut state, false, true));
        assert!(opens_on_setup(&mut state, true, true));
        assert!(!opens_on_setup(&mut state, true, true));
    }

    #[test]
    fn the_first_launch_leaves_a_mark() {
        let dir = tempfile::tempdir().expect("a folder");
        let state = dir.path().join("super-voice");
        assert!(first_launch(&state));
        assert!(!first_launch(&state));
    }

    /// The toast shows whatever macOS does with banners, with Allow typing
    /// when the permission stopped it, and the words' first line. Failure
    /// notices switched off leave it out.
    #[test]
    fn a_dictation_that_was_not_typed_shows_a_toast() {
        let toast_for = |reason, method| toast(&failed(reason, true), method);
        let shown =
            toast_for(TypingFailure::NotAllowed, Some(NotificationMethod::Auto)).expect("a toast");
        assert_eq!(
            shown,
            TypingToast {
                copied: true,
                allow_typing: true,
                quote: "“Call the landlord about the heater before Friday.”".into(),
            }
        );
        // Secure input, or anything else: no button, nothing to switch on.
        for reason in [TypingFailure::SecureInput, TypingFailure::Failed] {
            let shown = toast_for(reason, Some(NotificationMethod::Auto)).expect("a toast");
            assert!(!shown.allow_typing);
        }
        assert_eq!(
            toast_for(TypingFailure::NotAllowed, Some(NotificationMethod::Off)),
            None
        );
        // Not read yet: shown.
        assert!(toast_for(TypingFailure::NotAllowed, None).is_some());
    }

    /// Failure notices set to desktop notifications with macOS showing none:
    /// the toast is the notice. The same with typed notices, which can't be
    /// typed when typing is what failed.
    #[test]
    fn the_toast_is_the_fallback_when_notifications_are_off() {
        let mut state = SetupState::default();
        update(&mut state, Input::NotificationsRead(Some(false)));
        for method in [NotificationMethod::Desktop, NotificationMethod::Typed] {
            update(&mut state, Input::MethodRead(method));
            assert!(
                toast(
                    &failed(TypingFailure::NotAllowed, true),
                    state.notification_method
                )
                .is_some(),
                "{method:?}"
            );
        }
    }

    /// Back from System Settings: whether notifications are on is read again.
    #[test]
    fn notifications_are_read_again_when_the_window_is_back() {
        let mut state = SetupState::default();
        assert_eq!(
            update(&mut state, Input::OpenNotifications),
            [Effect::Open(notifications_url())]
        );
        assert_eq!(
            update(&mut state, Input::WindowActive),
            [Effect::ReadNotifications]
        );
    }

    #[test]
    fn a_changed_method_is_read_again() {
        let mut state = SetupState::default();
        assert_eq!(
            update(
                &mut state,
                Input::SettingChanged("notification_method".into())
            ),
            [Effect::ReadMethod]
        );
        assert!(update(&mut state, Input::SettingChanged("volume".into())).is_empty());
    }
}
