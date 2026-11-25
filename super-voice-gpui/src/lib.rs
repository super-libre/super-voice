// SPDX-License-Identifier: GPL-3.0-only
//! The Super Voice app, on GPUI with gpui-kit.
//!
//! What its pages show and what their buttons do is `super-voice-app-core`'s;
//! this crate draws it.

#[cfg(target_os = "macos")]
pub mod macos;

use gpui_kit::component::TitleBar;
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowOptions, div,
};

/// The window's id with the desktop: its Wayland app id, which the desktop
/// entry's `StartupWMClass` names.
pub const APP_ID: &str = "org.superlibre.SuperVoice";

/// Start the app: one window, until it is closed.
pub fn run() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            // A debug build keeps its session token apart from the installed
            // app's: the daemon binds a token to the binary it was granted
            // to, so sharing one entry would have each replace the other's.
            #[cfg(debug_assertions)]
            super_voice_app_core::daemon::client::use_scene_token();
            gpui_kit::init(cx);
            #[cfg(target_os = "macos")]
            macos::on_launch();
            if let Err(e) = gpui_kit::open_window(window_options(), cx, |_, cx| cx.new(|_| Shell)) {
                log::error!("could not open the window: {e:#}");
                cx.quit();
            }
        });
}

/// The main window's options.
#[must_use]
pub fn window_options() -> WindowOptions {
    let mut titlebar = TitleBar::title_bar_options();
    titlebar.title = Some("Super Voice".into());
    WindowOptions {
        titlebar: Some(titlebar),
        app_id: Some(APP_ID.to_string()),
        ..TitleBar::window_options()
    }
}

/// The window's content.
pub struct Shell;

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(TitleBar::new().child("Super Voice"))
    }
}
