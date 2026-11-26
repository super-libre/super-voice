// SPDX-License-Identifier: GPL-3.0-only
//! The window: its title bar, the pages (a sidebar of them, a rail of icons
//! or a drawer, by the window's width), the page shown, and a status bar.
//!
//! The shell owns what the pages share: the connection, each feature's
//! readiness, the daemon's events, which it hands to every page; the command
//! palette (Ctrl+K, Cmd+K on macOS); Alt+Left (and Cmd+[ on macOS), which goes
//! back from a detail.
//! While the daemon is down, the Connection page takes the place of the
//! pages that need it.

mod connection_page;
mod palette;
mod sidebar;
mod status_bar;
mod title_bar;

use std::time::Instant;

use gpui_kit::component::{ActiveTheme as _, Placement, Theme, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Context, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, Styled as _,
    Subscription, WeakEntity, Window, actions, div, px,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::connection::Input as ConnectionInput;
use super_voice_app_core::pages::readiness::Input as ReadinessInput;

use crate::connection::{Connection, ConnectionEvent};
use crate::daemon_events::{DaemonEvent, DaemonEvents};
use crate::pages::{self, Page, PageHandle};
use crate::readiness::Readiness;
use crate::shared::{Navigator, Shared};
use crate::ui::width::{self, WindowSize};

actions!(
    super_voice,
    [
        /// Open the command palette.
        OpenPalette,
        /// Go back from a detail shown alone to its list.
        Back,
        /// Record a new note, from anywhere in the window.
        NewNote,
    ]
);

/// The palette's keys: Cmd+K on macOS, Ctrl+K elsewhere.
pub const PALETTE_KEYS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// A new note's keys: Cmd+R on macOS, Ctrl+R elsewhere.
pub const NEW_NOTE_KEYS: &str = if cfg!(target_os = "macos") {
    "cmd-r"
} else {
    "ctrl-r"
};

/// The shell's keys: Ctrl+K (Cmd+K on macOS) for the palette, Ctrl+R
/// (Cmd+R) for a new note, Alt+Left for back, and Cmd+[ too on macOS, as
/// its apps go back.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(PALETTE_KEYS, OpenPalette, None),
        KeyBinding::new(NEW_NOTE_KEYS, NewNote, None),
        KeyBinding::new("alt-left", Back, None),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([KeyBinding::new("cmd-[", Back, None)]);
    }
}

/// The message keys the shell shows besides the pages' and headings' names.
#[cfg(test)]
pub const KEYS: &[&str] = &[
    "app-title",
    "daemon-connected",
    "daemon-connecting",
    "daemon-not-answering",
    "daemon-access-refused",
    "go-to-or-run",
    "palette-commands",
    "palette-go-to",
    "palette-nothing-matches",
    "palette-to-move",
    "palette-to-run",
    "palette-load-model",
    "unload-all-models",
    "new-note",
    "back-to",
    "transcription",
    "synthesis",
    "show-pages",
    "hide-pages",
    "status-access-refused",
    "status-daemon-not-running",
    "connection-refused-title",
    "connection-refused-body",
    "connection-restart-the-daemon",
    "connection-ask-again",
    "connection-not-running-title",
    "connection-not-running-body",
    "connection-start-the-daemon",
    "connection-try-now",
    "connection-switched-off-title",
    "connection-not-registered-title",
    "super-voice-s-background-service-is-switched",
    "super-voice-s-background-service-is-not",
    "open-login-items",
    "register",
    "retry",
    "retry-authorization",
    "status",
    "socket",
    "copy",
    "copy-command",
];

/// How long the window waits, asked to close, for the pages finishing their
/// work ([`crate::pages::PageView::closing`]) before it closes anyway.
const CLOSE_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// An event stream subscribed this long after the pages were told the
/// daemon answers is a new subscription, not the first one.
const RESUBSCRIBED_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// How far apart the pages left behind when the daemon answers read, one at
/// a time: all of them within a few seconds, so the palette offers every
/// page's commands, with none of the burst the daemon refuses (600 requests
/// a minute for one client; a Models page alone makes a dozen).
const CATCH_UP_EVERY: std::time::Duration = std::time::Duration::from_millis(400);

/// The window was asked to close, and waits for these pages to finish.
struct Closing {
    waiting: Vec<Page>,
    /// Closes the window after [`CLOSE_WAIT`] whatever the pages say.
    _give_up: gpui_kit::Task<()>,
}

/// The services the window runs on: whether the daemon answers, how ready
/// each feature is, and the daemon's events.
pub struct Services {
    pub connection: Entity<Connection>,
    pub readiness: Entity<Readiness>,
    pub events: Entity<DaemonEvents>,
}

/// The open window, for what reaches it from outside its elements: the menu
/// bar on macOS ([`crate::menus`]).
struct Current {
    shell: WeakEntity<Shell>,
    window: AnyWindowHandle,
}

impl Global for Current {}

/// Run `f` on the open window's shell: `false` when no window is open.
pub fn with_current(
    cx: &mut App,
    f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>),
) -> bool {
    let Some((shell, window)) = cx
        .try_global::<Current>()
        .map(|current| (current.shell.clone(), current.window))
    else {
        return false;
    };
    window
        .update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| f(shell, window, cx))
        })
        .is_ok_and(|updated| updated.is_ok())
}

/// The window's content.
pub struct Shell {
    /// The page picked. While the daemon is down and it needs the daemon,
    /// the Connection page shows in its place.
    page: Page,
    /// Every page, built when the window opens.
    pages: Vec<(Page, PageHandle)>,
    shared: Shared,
    events: Entity<DaemonEvents>,
    palette: palette::Palette,
    /// The drawer is open (narrow windows).
    drawer_open: bool,
    /// The window was asked to close and waits for pages to finish.
    closing: Option<Closing>,
    /// When the pages were last told the daemon answers.
    reached_at: Option<Instant>,
    /// Reads the pages left behind, one at a time.
    catch_up: Option<gpui_kit::Task<()>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// The window, showing `page`. With `follow_system`, light or dark
    /// follows the system's appearance as it changes.
    pub fn new(
        page: Page,
        services: Services,
        follow_system: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Services {
            connection,
            readiness,
            events,
        } = services;
        cx.set_global(crate::connection::Current(connection.clone()));
        let current = Current {
            shell: cx.weak_entity(),
            window: window.window_handle(),
        };
        cx.set_global(current);
        let shared = Shared {
            connection: connection.clone(),
            readiness: readiness.clone(),
            nav: Navigator(cx.weak_entity()),
        };
        let pages: Vec<(Page, PageHandle)> = Page::ALL
            .into_iter()
            .map(|page| (page, pages::build(page, &shared, window, cx)))
            .collect();

        let mut subscriptions = vec![
            cx.observe(&connection, |_, _, cx| cx.notify()),
            cx.observe(&readiness, |_, _, cx| cx.notify()),
            cx.subscribe_in(&connection, window, Self::on_connection),
            cx.subscribe_in(&events, window, Self::on_daemon_event),
        ];
        subscriptions.extend(pages.iter().map(|(_, handle)| handle.observe_by(cx)));
        if follow_system {
            crate::theme::follow_system(window, cx);
            subscriptions.push(cx.observe_window_appearance(window, |_, window, cx| {
                crate::theme::follow_system(window, cx);
            }));
        }
        // The desktop's close (Alt+F4, the traffic light) asks the pages
        // first, as the title bar's close button does.
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |shell, cx| shell.close(window, cx))
                .unwrap_or(true)
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            page,
            pages,
            shared,
            events,
            palette: palette::Palette::new(window, cx),
            drawer_open: false,
            closing: None,
            reached_at: None,
            catch_up: None,
            focus,
            _subscriptions: subscriptions,
        }
    }

    /// Show `page`.
    pub fn go(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        if self.drawer_open {
            self.drawer_open = false;
            window.close_sheet(cx);
        }
        // Only while the daemon answers: a page that doesn't need it opens
        // while it is down, and reads when it is back.
        if !self.shared.connection.read(cx).is_down() {
            self.handle(page).reach_if_stale(window, cx);
        }
        self.handle(page).shown(window, cx);
        cx.notify();
    }

    /// The daemon answers, first or again: the page shown and the pages the
    /// sidebar marks read now, the others one at a time after them, or when
    /// they are shown if that comes first.
    fn reach_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reached_at = Some(Instant::now());
        for (page, handle) in &self.pages {
            if *page == self.page || page.reads_while_hidden() {
                handle.reached(window, cx);
            } else {
                handle.mark_stale();
            }
        }
        self.catch_up = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(CATCH_UP_EVERY).await;
                let more = this.update_in(cx, |shell, window, cx| {
                    // Down again: what is left reads when it is back.
                    if shell.shared.connection.read(cx).is_down() {
                        return false;
                    }
                    let next = pages::next_to_catch_up(
                        shell
                            .pages
                            .iter()
                            .map(|(page, handle)| (*page, handle.is_stale())),
                    );
                    let Some(page) = next else {
                        return false;
                    };
                    shell.handle(page).clone().reached(window, cx);
                    true
                });
                if !matches!(more, Ok(true)) {
                    return;
                }
            }
        }));
    }

    /// The window is asked to close: `true` when it may close now. A page
    /// with work in flight (a take recording) holds it until it says it has
    /// finished, or [`CLOSE_WAIT`] is up; meanwhile it is shown, and a second
    /// ask changes nothing.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.closing.is_some() {
            return false;
        }
        let waiting: Vec<Page> = self
            .pages
            .clone()
            .into_iter()
            .filter(|(_, handle)| handle.closing(window, cx))
            .map(|(page, _)| page)
            .collect();
        let Some(&first) = waiting.first() else {
            return true;
        };
        self.go(first, window, cx);
        let give_up = cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(CLOSE_WAIT).await;
            log::warn!("Closing before every page finished its work");
            _ = cx.update(|window, _| window.remove_window());
        });
        self.closing = Some(Closing {
            waiting,
            _give_up: give_up,
        });
        false
    }

    /// Close the window as its close button does: at once, or once the pages
    /// holding it ([`Self::close`]) have finished.
    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close(window, cx) {
            window.remove_window();
        }
    }

    /// `page` finished the work it held the window open for: close it once
    /// no page waits.
    pub fn finished_closing(&mut self, page: Page, window: &mut Window, _: &mut Context<Self>) {
        let Some(closing) = self.closing.as_mut() else {
            return;
        };
        closing.waiting.retain(|waiting| *waiting != page);
        if closing.waiting.is_empty() {
            window.remove_window();
        }
    }

    fn handle(&self, page: Page) -> &PageHandle {
        &self
            .pages
            .iter()
            .find(|(p, _)| *p == page)
            .expect("every page is built")
            .1
    }

    /// Whether the Connection page shows in place of the page picked.
    fn shows_connection(&self, cx: &App) -> bool {
        self.page.needs_daemon() && self.shared.connection.read(cx).is_down()
    }

    /// The page the window shows, `None` while the Connection page does.
    fn page_shown(&self, cx: &App) -> Option<Page> {
        (!self.shows_connection(cx)).then_some(self.page)
    }

    fn on_connection(
        &mut self,
        _: &Entity<Connection>,
        event: &ConnectionEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ConnectionEvent::Reached => {
                self.events.update(cx, DaemonEvents::start);
                self.shared.readiness.update(cx, |readiness, cx| {
                    readiness.send(ReadinessInput::Reached, cx);
                });
                self.reach_pages(window, cx);
            }
            ConnectionEvent::Lost => {
                self.shared
                    .readiness
                    .update(cx, |readiness, cx| readiness.send(ReadinessInput::Lost, cx));
                // The Connection page says the daemon can't be reached; a
                // page's toast saying it again goes.
                crate::ui::toast::drop_unreachable(window, cx);
            }
            ConnectionEvent::Resubscribe => self.events.update(cx, DaemonEvents::restart),
        }
    }

    fn on_daemon_event(
        &mut self,
        _: &Entity<DaemonEvents>,
        DaemonEvent(event): &DaemonEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The event stream subscribed again while the daemon still
        // answered (it restarted between two pings): what the pages show
        // may have changed, and their events were missed. Its first
        // subscription, just after the daemon answered, is not news.
        if matches!(event, Event::Connected)
            && !self.shared.connection.read(cx).is_down()
            && self
                .reached_at
                .is_none_or(|at| at.elapsed() > RESUBSCRIBED_AFTER)
        {
            self.reach_pages(window, cx);
        }
        let connection = match event {
            Event::Connected => Some(ConnectionInput::Reached),
            Event::Blocked(reason) => Some(ConnectionInput::Refused(reason.clone())),
            Event::Disconnected => Some(ConnectionInput::StreamDropped),
            _ => None,
        };
        if let Some(input) = connection {
            self.shared
                .connection
                .update(cx, |connection, cx| connection.send(input, cx));
        }
        if let Event::Status(status) = event {
            let status = status.clone();
            self.shared.readiness.update(cx, |readiness, cx| {
                readiness.send(ReadinessInput::Heard(status), cx);
            });
        }
        for (_, handle) in &self.pages {
            handle.heard(event, window, cx);
        }
    }

    fn on_open_palette(&mut self, _: &OpenPalette, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(window, cx);
    }

    /// Open the command palette.
    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drawer_open {
            self.drawer_open = false;
            window.close_sheet(cx);
        }
        let groups = palette::entries(&self.pages, cx);
        self.palette.open(groups, window, cx);
    }

    /// Run what the palette's pick says: show a page, then run its command.
    fn run_pick(&mut self, pick: palette::Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            palette::Pick::GoTo(page) => self.go(page, window, cx),
            palette::Pick::Run(page, id) => {
                // Out of the palette's field, which is gone: a dialog the
                // command opens gives the focus back to the window, not to it.
                self.focus.focus(window, cx);
                self.go_and_run(page, &id, window, cx);
            }
        }
    }

    /// Show `page`, then run its command `id`, as a palette pick does.
    pub fn go_and_run(
        &mut self,
        page: Page,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go(page, window, cx);
        self.handle(page).clone().run(id, window, cx);
    }

    /// Ctrl+R (Cmd+R): record a new note, on the Notes page.
    fn on_new_note(&mut self, _: &NewNote, window: &mut Window, cx: &mut Context<Self>) {
        self.go_and_run(Page::Notes, pages::NEW_NOTE, window, cx);
    }

    fn on_back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.page_shown(cx) {
            self.handle(page).clone().back(window, cx);
        }
    }

    /// Open or close the drawer of pages (narrow windows).
    fn toggle_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drawer_open {
            self.drawer_open = false;
            window.close_sheet(cx);
            cx.notify();
            return;
        }
        self.drawer_open = true;
        let shell = cx.weak_entity();
        window.open_sheet_at(Placement::Left, cx, move |sheet, _, cx| {
            let Some(this) = shell.upgrade() else {
                return sheet;
            };
            let on_close = shell.clone();
            let nav = sidebar::Nav::read(this.read(cx), cx);
            sheet
                .size(width::DRAWER)
                // The sidebar fills it to its edges, in its own colour.
                .px_0()
                .bg(cx.theme().sidebar)
                .title(sidebar::brand(cx))
                .on_close(move |_, _, cx| {
                    _ = on_close.update(cx, |shell, cx| {
                        shell.drawer_open = false;
                        cx.notify();
                    });
                })
                .child(sidebar::drawer(&nav, shell.clone(), cx))
        });
        cx.notify();
    }

    /// Keep the window's overlays in step with its width: the drawer closes
    /// when the window grows out of narrow, and a toast takes a narrow
    /// window's width less 16 px a side.
    fn fit_overlays(&mut self, size: WindowSize, window: &mut Window, cx: &mut Context<Self>) {
        if size != WindowSize::Narrow && self.drawer_open {
            self.drawer_open = false;
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                window.close_sheet(cx);
                _ = weak.update(cx, |_, cx| cx.notify());
            });
        }
        let toast = (width::window_width(window) - px(32.)).min(px(382.));
        if cx.theme().notification.width != toast {
            window.defer(cx, move |_, cx| {
                Theme::global_mut(cx).notification.width = toast;
            });
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Through the hot-patch table, so a patch reaches everything drawn
        // from here (`just app-watch`).
        crate::hot::call(|| self.frame(window, cx))
    }
}

impl Shell {
    fn frame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let size = WindowSize::of(window);
        self.fit_overlays(size, window, cx);
        let content = if self.shows_connection(cx) {
            connection_page::render(&self.shared.connection, window, cx).into_any_element()
        } else {
            self.handle(self.page).view().into_any_element()
        };
        v_flex()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_open_palette))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(Self::on_new_note))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(title_bar::render(self, size, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .when(size != WindowSize::Narrow, |row| {
                        row.child(sidebar::sidebar(self, size, cx))
                    })
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .bg(cx.theme().background)
                            .child(content),
                    ),
            )
            .child(status_bar::render(self, size, cx))
            .into_any_element()
    }
}

/// The app's version, as the sidebar and the status bar show it.
#[must_use]
pub fn version() -> gpui_kit::SharedString {
    concat!("v", env!("CARGO_PKG_VERSION")).into()
}

#[cfg(feature = "screenshot")]
impl Shell {
    /// Made-up state for `just app-shot`: each page's own, then the scenes
    /// the shell draws itself (`palette`, `drawer`).
    pub fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        for (_, handle) in &self.pages {
            handle.pretend(scenes, window, cx);
        }
        if scenes.contains(&"palette") {
            self.open_palette(window, cx);
        }
        if scenes.contains(&"drawer") && WindowSize::of(window) == WindowSize::Narrow {
            self.toggle_drawer(window, cx);
        }
    }
}
