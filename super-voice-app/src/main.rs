// SPDX-License-Identifier: GPL-3.0-only
mod core;
// The daemon client, and the notes and clips, live in the core crate, which
// every app shares.
use super_voice_app_core::{clips, daemon, notes};
mod i18n;
mod state;
mod ui;

/// No CLI flags; the type exists only to satisfy `run_single_instance`'s
/// `CosmicFlags` bound. `action()` stays `None`, so a second launch
/// activates the running window rather than routing an action to it.
#[derive(Debug, Clone)]
pub struct Flags;

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = String;
    type Args = Vec<String>;
}

fn main() -> cosmic::iced::Result {
    super_voice_shared::logging::init();

    // Install the rustls crypto provider before any HTTP client is built —
    // the app's first direct download (the self-update installer binary,
    // via super-engine-forge) needs it.
    super_engine_forge::install_crypto_provider();

    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Settings for configuring the application window and iced runtime.
    let settings = cosmic::app::Settings::default().size_limits(
        cosmic::iced::Limits::NONE
            .min_width(360.0)
            .min_height(180.0),
    );

    // A scene opens at one size, so every shot of it frames the same window;
    // see `core::app::scene`.
    let scene = core::app::scene::requested();
    if scene.is_some() {
        // Before any daemon call, so the scene's token never replaces the
        // installed app's.
        daemon::client::use_scene_token();
    }
    let settings = if scene.is_some() {
        settings.size(core::app::scene::WINDOW_SIZE)
    } else {
        settings
    };

    // Run as a single-instance D-Bus-activated app. A second launch (e.g.
    // from the update notification's "Open Super Voice" action) activates
    // and focuses the existing window instead of opening a duplicate.
    //
    // Not for a scene, which opens beside a window the user may already
    // have rather than raising that one.
    if scene.is_none() {
        return cosmic::app::run_single_instance::<core::AppModel>(settings, Flags);
    }

    // A scene runs as a plain app.
    cosmic::app::run::<core::AppModel>(settings, Flags)
}
