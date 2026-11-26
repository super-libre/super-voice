// SPDX-License-Identifier: GPL-3.0-only
//! `super-voice-app`: the Super Voice app, on GPUI with gpui-kit.
//!
//! What its pages show and what their buttons do is `super-voice-app-core`'s;
//! this crate draws it. See [`shell`] for the window.
//!
//! One binary and no library, on purpose: hot-patching (`just app-watch`)
//! patches only the crate that holds `main`, so the app's code lives in it.

mod assets;
mod connection;
mod daemon_events;
mod effects;
mod hot;
#[cfg(target_os = "macos")]
mod macos;
mod menus;
mod pages;
mod readiness;
mod runtime;
mod shared;
mod shell;
#[cfg(feature = "screenshot")]
mod shot;
mod theme;
mod ui;

use gpui_kit::component::TitleBar;
use gpui_kit::{App, AppContext as _, Bounds, SharedString, WindowBounds, WindowOptions, px, size};
use super_voice_app_core::i18n;

use crate::connection::Connection;
use crate::daemon_events::DaemonEvents;
use crate::pages::Page;
use crate::readiness::Readiness;
pub use crate::shell::version;
use crate::shell::{Services, Shell};

/// The window's id with the desktop: its Wayland app id, which the desktop
/// entry's `StartupWMClass` names.
pub const APP_ID: &str = "org.superlibre.SuperVoice";

/// The window's size when it opens.
pub const WINDOW_SIZE: (f32, f32) = (1200., 760.);

fn main() {
    // A banner for the daemon, posted before anything else starts: no window,
    // no GPUI. See `macos::notifier`.
    #[cfg(target_os = "macos")]
    if macos::notifier::requested() {
        macos::notifier::run();
    }

    // `--shot <out.png> [page] [light|dark] [width] [scenes]`, with the
    // `screenshot` feature
    // (`just app-shot`).
    #[cfg(feature = "screenshot")]
    if std::env::args().nth(1).as_deref() == Some(shot::FLAG) {
        std::process::exit(shot::run());
    }

    super_voice_shared::logging::init();
    run();
}

/// Start the app: one window, until it is closed.
fn run() {
    // A debug build keeps its session token apart from the installed app's:
    // the daemon binds a token to the binary it was granted to, so sharing
    // one entry would have each replace the other's.
    #[cfg(debug_assertions)]
    super_voice_app_core::daemon::client::use_scene_token();

    gpui_kit::application()
        // Closing the window quits the app on macOS too, as System Settings
        // does: GPUI's default there keeps running with no window, and the
        // app has no way to open one again. The daemon runs on its own, so
        // nothing is lost. Linux already quits this way.
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed)
        .with_assets(assets::Assets)
        .run(|cx| {
            setup(cx);
            menus::install(cx);
            hot::connect(cx);
            #[cfg(target_os = "macos")]
            macos::on_launch();
            let services = Services {
                connection: cx.new(Connection::new),
                readiness: cx.new(|_| Readiness::new()),
                events: cx.new(|_| DaemonEvents::new()),
            };
            let opened = gpui_kit::open_window(window_options(cx), cx, |window, cx| {
                cx.new(|cx| Shell::new(Page::SttModels, services, true, window, cx))
            });
            if let Err(e) = opened {
                log::error!("could not open the window: {e:#}");
                cx.quit();
            }
        });
}

/// What the app sets up before its first window: the kit, the words in the
/// desktop's language, the theme, and the window's keys.
pub fn setup(cx: &mut App) {
    gpui_kit::init(cx);
    shell::bind_keys(cx);
    i18n::select(&i18n_embed::DesktopLanguageRequester::requested_languages());
    theme::install(cx);
}

/// The main window's options.
#[must_use]
pub fn window_options(cx: &App) -> WindowOptions {
    let mut titlebar = TitleBar::title_bar_options();
    titlebar.title = Some(t("app-title"));
    WindowOptions {
        titlebar: Some(titlebar),
        app_id: Some(APP_ID.to_string()),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)),
            cx,
        ))),
        window_min_size: Some(size(
            px(ui::width::MIN_WINDOW.0),
            px(ui::width::MIN_WINDOW.1),
        )),
        // The kit's title bar draws the window's own controls (Linux) or
        // leaves room for the traffic lights (macOS).
        #[cfg(target_os = "linux")]
        window_decorations: Some(gpui_kit::WindowDecorations::Client),
        ..TitleBar::window_options()
    }
}

/// The message `key` in the app's language.
#[must_use]
pub fn t(key: &str) -> SharedString {
    i18n::text(key, &[]).into()
}

/// [`t`], with `args` filled in, without the marks Fluent puts around each
/// value to keep its direction apart (U+2068, U+2069): the app's languages
/// all read left to right, and the marks would ride along, unseen, into text
/// the user copies.
#[must_use]
pub fn t_with(key: &str, args: &[(&str, &str)]) -> SharedString {
    i18n::text(key, args)
        .replace(['\u{2068}', '\u{2069}'], "")
        .into()
}
