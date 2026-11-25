// SPDX-License-Identifier: GPL-3.0-only
//! `super-voice-app-shot <out.png> [page] [light|dark]`: draw the app's window
//! off screen, through GPUI's headless renderer (wgpu on Linux, Metal on
//! macOS), and save it as a PNG. No display, no window on the desktop.
//!
//! The daemon is never asked: the status bar shows it connected. A new binary
//! asking the daemon for a token would bring up the consent dialog on the
//! desktop the shot is taken on. `just app-shot` runs this.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use gpui_kit::component::ThemeMode;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Bounds, HeadlessAppContext, WindowBounds, WindowOptions, px, size,
};
use super_voice_app_core::state::connection::DaemonStatus;
use super_voice_gpui::connection::Connection;
use super_voice_gpui::pages::Page;
use super_voice_gpui::shell::Shell;

fn main() -> ExitCode {
    super_voice_shared::logging::init_with(log::LevelFilter::Warn);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        let pages: Vec<&str> = Page::ALL.iter().map(|page| page.slug()).collect();
        eprintln!(
            "usage: super-voice-app-shot <out.png> [page] [light|dark]\npages: {}",
            pages.join(" ")
        );
        ExitCode::from(2)
    };
    let Some(out) = args.first().map(PathBuf::from) else {
        return usage();
    };
    let page = match args.get(1) {
        None => Page::SttModels,
        Some(slug) => match Page::from_slug(slug) {
            Some(page) => page,
            None => return usage(),
        },
    };
    let mode = match args.get(2).map(String::as_str) {
        None | Some("light") => ThemeMode::Light,
        Some("dark") => ThemeMode::Dark,
        Some(_) => return usage(),
    };
    match shoot(&out, page, mode) {
        Ok(()) => {
            println!("{}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("no screenshot: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn shoot(out: &std::path::Path, page: Page, mode: ThemeMode) -> anyhow::Result<()> {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(super_voice_gpui::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        super_voice_gpui::setup(cx);
        super_voice_gpui::theme::set_mode(mode, cx);
    });
    let (width, height) = super_voice_gpui::WINDOW_SIZE;
    let (handle, _) = cx.update(|cx| {
        let connection = cx.new(|_| Connection::fixed(DaemonStatus::Connected));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: gpui_kit::Point::default(),
                size: size(px(width), px(height)),
            })),
            show: false,
            focus: false,
            ..super_voice_gpui::window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(page, connection, false, window, cx))
        })
    })?;
    // Twice, with the background work between: the first frame asks for the
    // images, which load off the UI thread, and the second draws them.
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))?;
        cx.run_until_parked();
    }
    let image = cx.capture_screenshot(handle)?;
    image.save(out)?;
    Ok(())
}
