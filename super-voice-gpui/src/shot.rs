// SPDX-License-Identifier: GPL-3.0-only
//! `super-voice-app --shot <out.png> [page] [light|dark] [width] [scenes]`,
//! with the `screenshot` feature: draw the app's window off screen, through
//! GPUI's headless renderer (wgpu on Linux, Metal on macOS), and save it as
//! a PNG. No display, no window on the desktop.
//!
//! The daemon is never asked: a new binary asking the daemon for a token
//! would bring up the consent dialog on the desktop the shot is taken on.
//! Everything the window shows is made up instead ("pretend"): the shell's
//! here (the daemon connected, each feature's readiness, the GPU), each
//! page's in its `PageView::pretend`. `scenes`, comma-separated, pick
//! states: the shell's (`palette`, `drawer`, `refused`, `stopped`) and any a
//! page draws (`dictating`). `just app-shot` runs this.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::ThemeMode;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Bounds, HeadlessAppContext, WindowBounds, WindowOptions, px, size,
};
use super_voice_app_core::daemon::client::{ModelState, StageState};
use super_voice_app_core::pages::readiness::{Input, ReadinessState, update};
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::connection::DaemonStatus;
use super_voice_shared::models::protocol::{DownloadProgress, GpuInfo};

use crate::connection::Connection;
use crate::daemon_events::DaemonEvents;
use crate::pages::Page;
use crate::readiness::Readiness;
use crate::shell::{Services, Shell};

/// The flag that selects this mode.
pub const FLAG: &str = "--shot";

/// The frames drawn after the scenes are set, [`FRAME`] apart: long enough
/// for the kit's animations (150 ms) to end.
const ANIMATION_FRAMES: u32 = 6;
const FRAME: std::time::Duration = std::time::Duration::from_millis(60);

/// The shell's own scenes.
const SHELL_SCENES: &[&str] = &["palette", "drawer", "refused", "stopped"];

/// What the command line asks for.
struct Shot {
    out: PathBuf,
    page: Page,
    mode: ThemeMode,
    width: f32,
    scenes: Vec<String>,
}

/// Take the shot the command line asks for. The exit code: 0 with the PNG
/// written, 1 when it could not be drawn, 2 for a malformed command line.
pub fn run() -> i32 {
    super_voice_shared::logging::init_with(log::LevelFilter::Warn);
    let args: Vec<String> = std::env::args().skip(2).collect();
    let Some(shot) = parse(&args) else {
        let pages: Vec<&str> = Page::ALL.iter().map(|page| page.slug()).collect();
        eprintln!(
            "usage: super-voice-app {FLAG} <out.png> [page] [light|dark] [width] [scenes]\n\
             pages: {}\nshell scenes: {}",
            pages.join(" "),
            SHELL_SCENES.join(" ")
        );
        return 2;
    };
    match shoot(&shot) {
        Ok(()) => {
            println!("{}", shot.out.display());
            0
        }
        Err(e) => {
            eprintln!("no screenshot: {e:#}");
            1
        }
    }
}

fn parse(args: &[String]) -> Option<Shot> {
    let out = PathBuf::from(args.first()?);
    let page = match args.get(1) {
        None => Page::SttModels,
        Some(slug) => Page::from_slug(slug)?,
    };
    let mode = match args.get(2).map(String::as_str) {
        None | Some("light") => ThemeMode::Light,
        Some("dark") => ThemeMode::Dark,
        Some(_) => return None,
    };
    let width = match args.get(3).filter(|w| !w.is_empty()) {
        None => crate::WINDOW_SIZE.0,
        Some(w) => w
            .parse::<f32>()
            .ok()
            .filter(|w| *w >= crate::ui::width::MIN_WINDOW.0)?,
    };
    let scenes = args
        .get(4)
        .into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "none")
        .map(ToString::to_string)
        .collect();
    Some(Shot {
        out,
        page,
        mode,
        width,
        scenes,
    })
}

fn shoot(shot: &Shot) -> anyhow::Result<()> {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(crate::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        crate::setup(cx);
        crate::theme::set_mode(shot.mode, cx);
    });
    let scenes: Vec<&str> = shot.scenes.iter().map(String::as_str).collect();
    let status = if scenes.contains(&"refused") {
        DaemonStatus::Blocked("auth_denied (user_denied_cached)".into())
    } else if scenes.contains(&"stopped") {
        DaemonStatus::Error("Daemon HTTP listener not running. Start the daemon first.".into())
    } else {
        DaemonStatus::Connected
    };
    let down = status != DaemonStatus::Connected;
    let height = crate::WINDOW_SIZE.1;
    let (handle, shell) = cx.update(|cx| {
        let services = Services {
            connection: cx.new(|_| Connection::fixed(status)),
            readiness: cx.new(|_| {
                Readiness::fixed(if down {
                    ReadinessState::default()
                } else {
                    pretend_readiness()
                })
            }),
            events: cx.new(|_| DaemonEvents::new()),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: gpui_kit::Point::default(),
                size: size(px(shot.width), px(height)),
            })),
            show: false,
            focus: false,
            ..crate::window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(shot.page, services, false, window, cx))
        })
    })?;
    // A first frame, so what the scenes open knows the window's size.
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))?;
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.pretend(&scenes, window, cx));
    })?;
    // More frames, with the background work and some time between: a frame
    // asks for the images, which load off the UI thread, and the next draws
    // them; what a scene opens (the drawer, the palette) slides or fades in
    // for 150 ms, timed by the headless app's own clock, which only moves when
    // told to.
    for _ in 0..ANIMATION_FRAMES {
        cx.run_until_parked();
        cx.advance_clock(FRAME);
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))?;
    }
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))?;
    let image = cx.capture_screenshot(handle)?;
    image.save(&shot.out)?;
    Ok(())
}

/// The shell's made-up state: speech to text ready on Whisper's
/// `large-v3-turbo`, text to speech downloading `kokoro-82m`, an RTX 3090
/// with 6.1 of its 24 GiB in use.
fn pretend_readiness() -> ReadinessState {
    let mut state = ReadinessState::default();
    update(&mut state, Input::Reached);
    update(
        &mut state,
        Input::Viewed(
            Stage::Transcription,
            Ok(StageState {
                source: Some("github.com/super-libre/super-stt-whisper".into()),
                name: Some("Whisper".into()),
                enabled: true,
                model: Some("large-v3-turbo".into()),
                loaded: true,
                state: ModelState::Loaded,
                device: None,
            }),
        ),
    );
    update(
        &mut state,
        Input::Viewed(
            Stage::Synthesis,
            Ok(StageState {
                source: Some("github.com/super-libre/super-tts-kokoro".into()),
                name: Some("Kokoro".into()),
                enabled: false,
                model: Some("kokoro-82m".into()),
                loaded: false,
                state: ModelState::NotLoaded,
                device: None,
            }),
        ),
    );
    state.operations.start(
        Stage::Synthesis,
        super_voice_app_core::state::model_operations::ModelOperationState::Provisioning {
            target_model: "kokoro-82m".into(),
            progress: Box::new(pretend_progress()),
        },
    );
    update(
        &mut state,
        Input::GpusRead(vec![GpuInfo {
            name: "NVIDIA GeForce RTX 3090".into(),
            vendor: "nvidia".into(),
            total_bytes: 25_769_803_776,
            free_bytes: Some(19_220_840_653),
            used_bytes: Some(6_548_963_123),
            arch_target: Some("sm_86".into()),
        }]),
    );
    state
}

/// A download 42 % through, as the daemon reports one.
fn pretend_progress() -> DownloadProgress {
    serde_json::from_value(serde_json::json!({
        "model_name": "kokoro-82m", "source": "github.com/super-libre/super-tts-kokoro",
        "stage": 1, "current_file": "kokoro-v1_0.safetensors", "file_index": 1,
        "total_files": 3, "bytes_downloaded": 723_517_440_u64,
        "total_bytes": 1_702_887_424_u64, "percentage": 42.0,
        "status": "downloading", "started_at": "",
    }))
    .expect("a download's progress")
}
