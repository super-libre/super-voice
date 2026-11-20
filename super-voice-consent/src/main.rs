// SPDX-License-Identifier: GPL-3.0-only
//! Super Voice consent dialog — a small floating libcosmic window that the
//! daemon spawns when an app asks to authenticate.
//!
//! The daemon spawns this binary with env vars carrying the request details
//! (the names are [`contract`]'s, shared with Super TTS):
//!
//! - `SUPER_VOICE_AUTH_APP_NAME` — declared (untrusted) app name from the
//!   request. Shown for a native caller, never for a web one — see [`Caller`].
//! - `SUPER_VOICE_AUTH_SCOPES`   — space-separated scope set (e.g. `transcribe status`)
//! - `SUPER_VOICE_AUTH_EXE_PATH` — peer `/proc/<pid>/exe` (trusted, kernel-resolved)
//! - `SUPER_VOICE_AUTH_FLATPAK_APP_ID` — set only when the caller is inside a flatpak
//! - `SUPER_VOICE_AUTH_WEB_ORIGIN` — set only when the caller reached the daemon
//!   over its TCP listener, carrying the browser-reported origin. The daemon
//!   sets this *or* `SUPER_VOICE_AUTH_EXE_PATH`, never both, and this one wins.
//!
//! The user clicks Allow or Deny. The dialog writes one of `allow`, `deny`,
//! or `dismissed` to stdout (newline-terminated) and exits.
//!
//! ## Floating-window behavior
//!
//! Where the compositor offers it, the dialog is rendered as a Wayland
//! **layer-shell** surface (`Layer::Overlay`,
//! `KeyboardInteractivity::Exclusive`, `anchor: Anchor::empty()`). Tiling
//! compositors (cosmic-comp, sway, Hyprland, niri, etc.) treat layer-shell
//! surfaces as overlays — they are NOT subject to tiling logic — so the
//! consent dialog floats everywhere by construction. This is the same
//! protocol cosmic-osd uses for the polkit-auth dialog and the
//! volume/brightness OSDs.
//!
//! GNOME's compositor has no layer-shell. A layer surface asked of it never
//! reaches the screen, and the request would sit unanswered until the
//! daemon gave up on it. So the dialog first asks the compositor which
//! protocols it offers, and where layer-shell isn't one of them it opens an
//! ordinary window instead: see [`Surface`].
//!
//! For X11 sessions, layer-shell isn't available; libcosmic falls back
//! to a regular window. On X11 tiling WMs you'd add a per-class float
//! rule using the `WM_CLASS` `super-voice-consent`.

use cosmic::iced::event::{self, listen_with};
use cosmic::iced::platform_specific::shell::commands::corner_radius::corner_radius;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer,
};
use cosmic::iced::runtime::platform_specific::wayland::CornerRadius;
use cosmic::iced::runtime::platform_specific::wayland::layer_surface::SctkLayerSurfaceSettings;
use cosmic::iced::window::Id as SurfaceId;
use cosmic::iced::{Limits, Size, Subscription};
use cosmic::prelude::*;
use cosmic::surface::{action as surface_action, surface_task};
use cosmic::widget::{button, dialog, icon, text};
use std::io::Write;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use super_voice_shared::SUPER_VOICE;
use super_voice_shared::consent::contract;
// The decision payloads written to stdout right before exit.
use contract::{ALLOW, DENY, DISMISSED};

const APP_ID: &str = "ai.menjivar.super-voice-consent";

const QUESTION_ICON: &[u8] = include_bytes!("../resources/icons/phosphor/question.svg");

/// Floor for the autosized surface, in logical pixels. Only applies for the
/// frames before `autosize` measures the dialog.
const MIN_SURFACE_DIM: f32 = 1.0;

/// The dialog's question, also the title of the window it gets where there
/// is no layer-shell, for the compositor's window switcher.
const TITLE: &str = "Allow access to Super Voice?";

/// The size a window opens at, before `autosize` fits it to the dialog.
const WINDOW_SIZE: Size = Size::new(480.0, 360.0);

/// The layer-shell protocol's global, as compositors advertise it.
const LAYER_SHELL: &str = "zwlr_layer_shell_v1";

/// How the dialog reaches the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    /// A layer-shell overlay, above every window: the default, and what
    /// every compositor with layer-shell gets.
    Overlay,
    /// An ordinary window, on a Wayland compositor without layer-shell:
    /// GNOME. GNOME brings it to the front and gives it the keyboard, over
    /// whatever window had them, but unlike an overlay it can be covered or
    /// switched away from. It reaches the screen, where an overlay would
    /// not.
    Window,
}

impl Surface {
    /// The surface for a compositor that does (`Some(true)`) or doesn't
    /// (`Some(false)`) offer layer-shell. `None` is no Wayland compositor to
    /// ask, an X11 session, which keeps the overlay path and libcosmic's own
    /// fallback for it.
    fn for_layer_shell(offered: Option<bool>) -> Self {
        match offered {
            Some(false) => Self::Window,
            Some(true) | None => Self::Overlay,
        }
    }
}

/// Whether a compositor advertising `interfaces` offers layer-shell.
fn offers_layer_shell<'a>(mut interfaces: impl Iterator<Item = &'a str>) -> bool {
    interfaces.any(|interface| interface == LAYER_SHELL)
}

/// Ask the Wayland compositor whether it offers layer-shell. `None` when
/// there is no Wayland compositor to ask.
fn compositor_offers_layer_shell() -> Option<bool> {
    use cosmic::cctk::wayland_client::globals::{GlobalListContents, registry_queue_init};
    use cosmic::cctk::wayland_client::protocol::wl_registry;
    use cosmic::cctk::wayland_client::{Connection, Dispatch, QueueHandle};

    /// Receives nothing: one look at the globals the compositor lists when
    /// first asked is all the dialog needs.
    struct Probe;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    let connection = Connection::connect_to_env().ok()?;
    let (globals, _queue) = registry_queue_init::<Probe>(&connection).ok()?;
    Some(
        globals
            .contents()
            .with_list(|list| offers_layer_shell(list.iter().map(|g| g.interface.as_str()))),
    )
}

/// No compositor-side corner clip. The state the surface starts in, and the
/// only safe one until it has a size — see [`ConsentApp::sync_corner_radius`].
const SQUARE_CORNERS: CornerRadius = CornerRadius {
    top_left: 0,
    top_right: 0,
    bottom_left: 0,
    bottom_right: 0,
};

/// Stable id for the autosize widget that wraps the dialog body.
/// `widget::autosize::autosize` measures its child to size the parent
/// layer-shell surface; without it the surface is 0×0.
static AUTOSIZE_ID: LazyLock<cosmic::widget::Id> =
    LazyLock::new(|| cosmic::widget::Id::new("super-voice-consent-autosize"));

/// The value of the request variable `name` (one of [`contract`]'s), under
/// Super Voice's prefix.
fn request_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(SUPER_VOICE.env(name))
}

/// Set the moment the user picks Allow or Deny. Used by the
/// signal handler to skip an extra "dismissed" line if the user
/// already decided.
static DECIDED: AtomicBool = AtomicBool::new(false);

/// How long the dialog stays up before dismissing itself.
///
/// Deliberately longer than the daemon's own one-minute wait, so this
/// never truncates a decision the daemon would still have accepted —
/// it is a backstop for a dialog that outlived whoever asked for it,
/// not a second deadline. The daemon reaps the helper itself, but only
/// while the request that spawned it is alive; nothing can kill a
/// dialog whose daemon has since exited. Left to wait forever such a
/// dialog holds an exclusive-keyboard layer surface and ~40 MB.
const DISMISS_AFTER: std::time::Duration = std::time::Duration::from_secs(90);

#[derive(Clone, Debug)]
enum Message {
    Allow,
    Deny,
    /// The compositor configured our surface to `Size`. Carries the id so a
    /// stray event for another surface can't drive our corner radius.
    Resized(SurfaceId, Size),
}

struct ConsentApp {
    core: cosmic::Core,
    surface: Surface,
    /// The overlay the dialog is drawn on, or the window, by its id.
    surface_id: SurfaceId,
    app_name: String,
    scopes: Vec<String>,
    caller: Caller,
}

impl cosmic::Application for ConsentApp {
    type Executor = cosmic::executor::Default;
    type Flags = AuthRequestPayload;
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        mut core: cosmic::Core,
        flags: Self::Flags,
    ) -> (Self, cosmic::Task<cosmic::Action<Self::Message>>) {
        if flags.surface == Surface::Window {
            // Just the dialog, with no header bar over it: its title is the
            // question, and Allow and Deny are the only ways out besides
            // closing the window.
            core.window.show_headerbar = false;
            let surface_id = core.main_window_id().unwrap_or(SurfaceId::RESERVED);
            let mut app = Self {
                core,
                surface: Surface::Window,
                surface_id,
                app_name: flags.app_name,
                scopes: flags.scopes,
                caller: flags.caller,
            };
            let task = app.set_window_title(TITLE.to_string(), surface_id);
            return (app, task);
        }

        let surface_id = SurfaceId::unique();

        // We draw a layer-shell overlay, not an application window, so the
        // dialog belongs to the system interface: its translucency follows the
        // theme's "frosted system interface" toggle rather than the one for
        // regular windows.
        core.set_app_type(cosmic::core::AppType::System);

        // Spawn a layer-shell overlay surface. Tiling compositors do
        // not tile layer-shell surfaces, so this is the protocol-level
        // way to guarantee the dialog floats. KeyboardInteractivity is
        // Exclusive so the dialog grabs all keyboard input until the
        // user makes a choice (matches cosmic-osd's polkit dialog).
        //
        // Routed through `cosmic::surface` rather than the raw
        // `get_layer_surface` command so libcosmic tracks the surface and owns
        // its frosted-glass blur, re-applying it when the theme changes. A
        // surface spawned directly is invisible to that bookkeeping, which
        // leaves it translucent with nothing blurred behind it whenever the
        // theme has frosted glass on.
        let task = surface_task(surface_action::app_layer_shell::<Self>(
            |_| surface_action::LiveSettings {
                // Inherit the theme's blur decision.
                blur: None,
                // Corners start square and are rounded once the surface has a
                // size. libcosmic would otherwise round a tracked layer
                // surface to the theme's radius the moment it's created —
                // before the compositor has configured any size for it — and
                // cosmic-comp answers a radius wider than the surface it's
                // clipping with `cosmic_corner_radius_layer_v1: error 1:
                // corner radius too large`, killing the client. Raising the
                // surface's `size_limits` floor does not help; the request
                // goes out ahead of the first configure either way.
                corners: Some(SQUARE_CORNERS),
                padding: None,
            },
            move |_: &mut Self| SctkLayerSurfaceSettings {
                id: surface_id,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: Anchor::empty(),
                namespace: "stt-consent".into(),
                layer: Layer::Overlay,
                size: None,
                size_limits: Limits::NONE
                    .min_width(MIN_SURFACE_DIM)
                    .min_height(MIN_SURFACE_DIM),
                ..Default::default()
            },
            // No dedicated view: libcosmic falls back to `view_window`.
            None,
        ));

        (
            Self {
                core,
                surface: Surface::Overlay,
                surface_id,
                app_name: flags.app_name,
                scopes: flags.scopes,
                caller: flags.caller,
            },
            task,
        )
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // The window, where there is one, is the dialog. With an overlay
        // there is no main window, and the overlay is rendered by
        // view_window; returning a tiny placeholder keeps libcosmic happy on
        // the rare build flavors that hit this path.
        match self.surface {
            Surface::Window => self.dialog(),
            Surface::Overlay => cosmic::widget::text::body("").into(),
        }
    }

    fn view_window(&self, id: SurfaceId) -> Element<'_, Self::Message> {
        if id != self.surface_id {
            return cosmic::widget::text::body("").into();
        }
        self.dialog()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // `Opened` covers the compositor's first configure, `Resized` any
        // later one — a font-scale change, say. Both feed the corner radius.
        listen_with(|ev, _status, id| match ev {
            event::Event::Window(
                cosmic::iced::window::Event::Opened { size, .. }
                | cosmic::iced::window::Event::Resized(size),
            ) => Some(Message::Resized(id, size)),
            _ => None,
        })
    }

    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Self::Message>> {
        let decision = match message {
            Message::Allow => ALLOW,
            Message::Deny => DENY,
            Message::Resized(id, size) => return self.sync_corner_radius(id, size),
        };
        // Write the decision to stdout and exit; the compositor tears the
        // surface down when the process goes away. Returning a destroy task
        // instead would never run — `emit_and_exit` diverges before the
        // runtime gets a chance to execute it.
        emit_and_exit(decision);
    }
}

impl ConsentApp {
    /// The dialog: the question, who is asking, what it would allow, and
    /// Allow and Deny.
    fn dialog(&self) -> Element<'_, Message> {
        // The surface starts transparent; using
        // `widget::dialog::dialog()` gives us the proper themed
        // background, padding, and button layout — same widget the
        // cosmic-osd polkit dialog uses.
        // What the user is being asked to approve, named the way each kind of
        // caller is best named.
        //
        // A native caller is named by `app_name`, with its verified path on the
        // line below — the name is self-reported, and the path is what the user
        // is really approving.
        //
        // A web caller is named by its origin, and `app_name` is not shown at
        // all. The origin is the only thing about a web caller that anything
        // vouches for, and it is also what the user recognises: browsers say
        // "example.com wants to use your microphone" for the same reason. Since
        // a page could otherwise put a string of its own choosing in front of
        // the user, leaving `app_name` out is also what stops a site from
        // labelling itself something it is not.
        let body_text = sentence(&self.caller, &self.app_name);

        let permission_lines = super_voice_shared::consent::permissions_for_scopes(&self.scopes);

        let mut bullet_column =
            cosmic::widget::column::with_capacity(permission_lines.len()).spacing(6);
        for line in permission_lines {
            bullet_column = bullet_column.push(bullet_row(line));
        }

        let mut control = cosmic::widget::column::with_capacity(3).spacing(12);

        // The verified identity, on its own line under the sentence. Only a
        // native caller gets one: for a web caller the sentence already *is*
        // the verified identity, and repeating the origin underneath would
        // read as a second, corroborating fact when there is only one.
        if let Caller::Native {
            exe_path,
            flatpak_app_id,
        } = &self.caller
        {
            control = control.push(text::body(match flatpak_app_id {
                // Name the app the user installed. Its executable path lives
                // inside its sandbox, so it identifies nothing here.
                Some(id) => format!("Flatpak app:  {id}"),
                None => format!("Executable:  {exe_path}"),
            }));
        }

        control = control.push(
            cosmic::widget::column::with_capacity(2)
                .push(text::heading("This will allow it to:"))
                .push(bullet_column)
                .spacing(6),
        );

        // A native caller is named by the kernel and cannot lie about it. A web
        // caller is named by its browser, and the user should be told that
        // before they grant anything — it is the difference between a checked
        // identity and a reported one.
        if matches!(self.caller, Caller::Web { .. }) {
            control = control.push(text::caption(
                "Your browser reports which website this is. That is a weaker check \
                 than Super Voice does for installed programs.",
            ));
        }

        let dialog_widget = dialog::dialog()
            // `Container::Dialog` only honours the theme's translucency when
            // the dialog is *not* an overlay — an overlay sits on a dimmed
            // backdrop inside an app window, where a see-through panel would
            // read as a rendering bug. Ours is a standalone surface with the
            // desktop behind it, so it takes the frosted treatment instead.
            .is_overlay(false)
            .title(TITLE)
            .body(body_text)
            .icon(
                icon::from_svg_bytes(QUESTION_ICON)
                    .symbolic(true)
                    .icon()
                    .size(64),
            )
            .control(control)
            .primary_action(button::suggested("Allow").on_press(Message::Allow))
            .secondary_action(button::destructive("Deny").on_press(Message::Deny));

        // `autosize` measures the dialog widget and resizes the surface to
        // match. Without it an overlay stays 0×0 and the dialog is
        // invisible, and a window keeps the size it opened at.
        cosmic::widget::autosize::autosize(dialog_widget, AUTOSIZE_ID.clone())
            .min_width(MIN_SURFACE_DIM)
            .min_height(MIN_SURFACE_DIM)
            .into()
    }

    /// Match the compositor's corner clip to the rounded border the dialog
    /// widget draws, now that the surface has a size to measure against.
    ///
    /// Without this the surface stays a square the dialog is painted into, and
    /// its background shows through outside the rounded border at each corner.
    /// The radius is clamped to half the shorter side: cosmic-comp rejects
    /// anything wider than the surface it's clipping, and rejection means a
    /// protocol error that kills the process, not a clamp on its end. Same
    /// shape as the cosmic-osd indicator, which rounds on `Msg::Size`.
    fn sync_corner_radius(
        &self,
        id: SurfaceId,
        size: Size,
    ) -> cosmic::Task<cosmic::Action<Message>> {
        // Only an overlay is clipped by cosmic-comp's corner protocol. A
        // window's corners are its own to draw.
        if id != self.surface_id || self.surface != Surface::Overlay {
            return cosmic::Task::none();
        }
        // Degenerate sizes are the pre-configure state; stay square. The
        // finite check also rejects NaN, which would otherwise slip past the
        // comparison and clamp to nothing.
        let limit = size.width.min(size.height) / 2.0;
        if !limit.is_finite() || limit < 1.0 {
            return cosmic::Task::none();
        }
        // `radius_m` is what `Container::Dialog` rounds its own border to, so
        // the clip lands exactly on the edge the user sees.
        let radii = cosmic::theme::active().cosmic().corner_radii.radius_m;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let clamp = |r: f32| r.min(limit).max(0.0).round() as u32;
        corner_radius(
            self.surface_id,
            Some(CornerRadius {
                top_left: clamp(radii[0]),
                top_right: clamp(radii[1]),
                bottom_right: clamp(radii[2]),
                bottom_left: clamp(radii[3]),
            }),
        )
        .discard()
    }
}

/// A program that runs other people's code inside its own process: GNOME
/// Shell runs every extension in `gnome-shell`. The daemon knows a caller by
/// its executable, so for one of these it knows the host and nothing more:
/// any of the code it runs may be asking, under any name. The dialog names
/// the host, which it can vouch for, and not the name in the request.
struct Host {
    /// The executable's file name.
    binary: &'static str,
    /// What the user knows it as.
    name: &'static str,
}

const HOSTS: &[Host] = &[Host {
    binary: "gnome-shell",
    name: "GNOME Shell",
}];

impl Caller {
    /// The host this caller is, when it is one. A sandboxed caller never is:
    /// its path names a file inside its sandbox.
    fn host(&self) -> Option<&'static Host> {
        let Caller::Native {
            exe_path,
            flatpak_app_id: None,
        } = self
        else {
            return None;
        };
        // The kernel marks a binary replaced since it started, as after an
        // upgrade, with ` (deleted)`.
        let path = exe_path.strip_suffix(" (deleted)").unwrap_or(exe_path);
        let name = std::path::Path::new(path).file_name()?.to_str()?;
        HOSTS.iter().find(|host| host.binary == name)
    }
}

/// The sentence naming who asks. A host is named for what it is, never by
/// the name in the request, which anything it runs could send.
fn sentence(caller: &Caller, app_name: &str) -> String {
    if let Some(host) = caller.host() {
        return format!("{} wants access to Super Voice.", host.name);
    }
    match caller {
        Caller::Native { .. } => {
            let name = if app_name.is_empty() {
                "An application"
            } else {
                app_name
            };
            format!("{name} wants access to Super Voice.")
        }
        Caller::Web { origin } => format!("{origin} wants access to Super Voice."),
    }
}

/// Render one bullet line. Uses a Row so wrapped text hangs under
/// itself instead of slipping behind the bullet character.
fn bullet_row(line: &str) -> Element<'_, Message> {
    cosmic::widget::row::with_capacity(2)
        .push(text::body("•"))
        .push(text::body(line))
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Start)
        .into()
}

/// Write the decision to stdout, flush, then exit with `_exit(2)`.
/// Returns `!` so it can stand in for the `Task<...>` an
/// `Application::update` would normally return.
fn emit_and_exit(decision: &str) -> ! {
    DECIDED.store(true, Ordering::SeqCst);
    {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        let _ = writeln!(handle, "{decision}");
        let _ = handle.flush();
    }
    std::process::exit(0);
}

/// Async-signal-safe SIGTERM/SIGINT handler. Writes "dismissed\n" to
/// stdout (fd 1) using `write(2)`, then exits with `_exit(2)` —
/// both AS-Safe per POSIX. Skips the write if a button decision has
/// already been emitted.
extern "C" fn handle_termination_signal(_signum: libc::c_int) {
    if DECIDED.load(Ordering::SeqCst) {
        unsafe {
            libc::_exit(0);
        }
    }
    let msg = b"dismissed\n";
    unsafe {
        libc::write(1, msg.as_ptr().cast::<libc::c_void>(), msg.len());
        libc::_exit(0);
    }
}

fn install_termination_handlers() {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            handle_termination_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            handle_termination_signal as *const () as libc::sighandler_t,
        );
    }
}

/// Dismiss the dialog if nobody has decided within [`DISMISS_AFTER`].
///
/// Unlike the auto-approve timer below this is compiled into release
/// builds: it only ever denies, so it cannot be used to wave a consent
/// prompt through. Fail-closed is also the right answer when it does
/// fire — reaching the deadline means no human answered.
fn spawn_dismiss_timer() {
    std::thread::spawn(|| {
        std::thread::sleep(DISMISS_AFTER);
        // Claim the decision. If someone else got there first, their
        // verdict is already on stdout and must not be followed by
        // ours — the daemon reads one line and we would be racing to
        // define it.
        if DECIDED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            unsafe { libc::_exit(0) }
        }
        // Same async-signal-safe write + `_exit` as the SIGTERM handler,
        // rather than trying to unwind the iced event loop from a thread
        // that does not own it.
        let msg = b"dismissed\n";
        unsafe {
            libc::write(1, msg.as_ptr().cast::<libc::c_void>(), msg.len());
            libc::_exit(0);
        }
    });
}

/// If `SUPER_VOICE_AUTH_AUTO_APPROVE_AFTER_MS` is set to a parseable u64,
/// spawn a background thread that sleeps for that many milliseconds
/// and then writes "allow\n" to stdout + `_exit(0)`. Lets you (or a
/// test runner) actually see the dialog render for that long before
/// it auto-approves itself.
///
/// This is intended for integration tests / CI smoke runs, NOT for
/// production. The auto-approval bypasses any user input, so the whole
/// path is compiled out of release builds — a shipped consent gate must
/// never be able to self-approve from an env var (Tier 1 #30). `cargo test`
/// builds with `debug_assertions` on, so the smoke tests keep working.
#[cfg(debug_assertions)]
fn maybe_spawn_auto_approve_timer() {
    let var = SUPER_VOICE.env(contract::AUTO_APPROVE_AFTER_MS);
    let Ok(raw) = std::env::var(&var) else {
        return;
    };
    let Ok(ms) = raw.parse::<u64>() else {
        log::warn!("{var}={raw:?} is not a valid u64; ignoring");
        return;
    };
    log::info!("{var}={ms}; auto-approving after {ms}ms");
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        // Mark decided so the SIGTERM/atexit paths don't also write
        // "dismissed" on top of our "allow".
        DECIDED.store(true, Ordering::SeqCst);
        // Async-signal-safe write + _exit, same shape as the SIGTERM
        // handler. Avoids contending with the iced event loop.
        let msg = b"allow\n";
        unsafe {
            libc::write(1, msg.as_ptr().cast::<libc::c_void>(), msg.len());
            libc::_exit(0);
        }
    });
}

/// Release builds never auto-approve — the env-var bypass is debug/test only.
#[cfg(not(debug_assertions))]
fn maybe_spawn_auto_approve_timer() {}

/// Who the daemon says is asking.
///
/// Mirrors the daemon's own `PeerIdentity`, and for the same reason: the two
/// kinds warrant different sentences. "Allow this program" and "allow this
/// website" are not the same question, and the second comes with a weaker
/// guarantee behind it — so the dialog has to know which one it is asking
/// rather than infer it from an empty field.
enum Caller {
    /// A program on this machine, named by the path the kernel resolved.
    Native {
        exe_path: String,
        /// Set only for a sandboxed caller. Its `exe_path` is resolved inside
        /// its own sandbox, so showing that path would name a file the user
        /// cannot go and look at, and one that every other flatpak could
        /// equally present.
        flatpak_app_id: Option<String>,
    },
    /// A web page, named by the origin its browser reported.
    Web { origin: String },
}

struct AuthRequestPayload {
    app_name: String,
    scopes: Vec<String>,
    caller: Caller,
    /// How the dialog reaches the screen: decided once, before the app
    /// starts, because a window needs settings an overlay doesn't.
    surface: Surface,
}

fn read_env(surface: Surface) -> AuthRequestPayload {
    AuthRequestPayload {
        surface,
        app_name: request_var(contract::APP_NAME).unwrap_or_else(|_| "<unknown app>".to_string()),
        scopes: request_var(contract::SCOPES)
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        caller: read_caller(),
    }
}

/// Which kind of caller the daemon is asking about.
///
/// `SUPER_VOICE_AUTH_WEB_ORIGIN` wins when present because the daemon sets
/// exactly one of the two — see its `ask_user_for_consent`. Reading it first
/// means a stale `SUPER_VOICE_AUTH_EXE_PATH` inherited from the environment can
/// never turn a website into an executable in the sentence the user reads.
fn read_caller() -> Caller {
    if let Ok(origin) = request_var(contract::WEB_ORIGIN)
        && !origin.is_empty()
    {
        return Caller::Web { origin };
    }
    Caller::Native {
        exe_path: request_var(contract::EXE_PATH).unwrap_or_else(|_| "<unknown path>".to_string()),
        flatpak_app_id: request_var(contract::FLATPAK_APP_ID)
            .ok()
            .filter(|id| !id.is_empty()),
    }
}

fn main() -> cosmic::iced::Result {
    super_voice_shared::logging::init();

    install_termination_handlers();
    spawn_dismiss_timer();
    maybe_spawn_auto_approve_timer();

    let surface = Surface::for_layer_shell(compositor_offers_layer_shell());
    log::info!("Showing the consent dialog as {surface:?}");
    let payload = read_env(surface);

    let settings = match surface {
        // `no_main_window(true)` because we use a layer-shell surface
        // instead of the standard auto-created xdg_toplevel main window.
        // The surface is created from `init()` via `get_layer_surface(...)`.
        Surface::Overlay => cosmic::app::Settings::default()
            .no_main_window(true)
            .exit_on_close(true),
        // The main window is the dialog, fitted to it by `autosize` rather
        // than resized by hand. It stays transparent, so the corners outside
        // the dialog's rounded border show the desktop behind it.
        Surface::Window => cosmic::app::Settings::default()
            .size(WINDOW_SIZE)
            .resizable(None)
            .exit_on_close(true),
    };

    let result = cosmic::app::run::<ConsentApp>(settings, payload);

    // If we reach this without DECIDED being set, the user dismissed
    // the dialog without picking a button. Emit "dismissed".
    if !DECIDED.load(Ordering::SeqCst) {
        let _ = writeln!(std::io::stdout(), "{DISMISSED}");
        let _ = std::io::stdout().flush();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{Caller, Surface, offers_layer_shell, sentence};

    fn native(exe_path: &str) -> Caller {
        Caller::Native {
            exe_path: exe_path.into(),
            flatpak_app_id: None,
        }
    }

    /// GNOME Shell runs every extension in one process, so the daemon can't
    /// tell which asks, or vouch for the name it gives. The dialog names
    /// GNOME Shell, which it can vouch for.
    #[test]
    fn a_gnome_shell_caller_is_named_gnome_shell() {
        for exe in ["/usr/bin/gnome-shell", "/usr/bin/gnome-shell (deleted)"] {
            assert_eq!(
                sentence(&native(exe), "Super Voice for GNOME"),
                "GNOME Shell wants access to Super Voice."
            );
        }
    }

    /// Every other program is named as before: by the name it gives, with
    /// its executable under it.
    #[test]
    fn other_callers_keep_their_wording() {
        for exe in [
            "/usr/local/bin/super-voice-app",
            "/opt/gnome-shell-tools/helper",
        ] {
            let caller = native(exe);
            assert_eq!(
                sentence(&caller, "Notes"),
                "Notes wants access to Super Voice."
            );
        }
        // A flatpak's path is inside its sandbox, so it names no host.
        let sandboxed = Caller::Native {
            exe_path: "/app/bin/gnome-shell".into(),
            flatpak_app_id: Some("org.example.Shell".into()),
        };
        assert_eq!(
            sentence(&sandboxed, "Shell"),
            "Shell wants access to Super Voice."
        );
        let web = Caller::Web {
            origin: "https://example.com".into(),
        };
        assert_eq!(
            sentence(&web, "ignored"),
            "https://example.com wants access to Super Voice."
        );
    }

    /// GNOME lists no layer-shell; cosmic-comp, sway and the rest do.
    #[test]
    fn layer_shell_is_found_among_the_compositors_globals() {
        let gnome = [
            "wl_compositor",
            "xdg_wm_base",
            "wl_seat",
            "xdg_activation_v1",
        ];
        let cosmic = ["wl_compositor", "xdg_wm_base", "zwlr_layer_shell_v1"];
        assert!(!offers_layer_shell(gnome.into_iter()));
        assert!(offers_layer_shell(cosmic.into_iter()));
    }

    /// Only a compositor known to lack layer-shell gets a window. Where
    /// there's no Wayland compositor to ask, an X11 session, the overlay path
    /// and libcosmic's own fallback for it stay as they were.
    #[test]
    fn only_a_compositor_without_layer_shell_gets_a_window() {
        assert_eq!(Surface::for_layer_shell(Some(false)), Surface::Window);
        assert_eq!(Surface::for_layer_shell(Some(true)), Surface::Overlay);
        assert_eq!(Surface::for_layer_shell(None), Surface::Overlay);
    }
}
