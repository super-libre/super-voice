// SPDX-License-Identifier: GPL-3.0-only
//! What macOS has to allow, as a GPUI model: the core's `pages::setup` run
//! for the window. The Typed card on Speech to text › Models, the Finish
//! setup page and the Settings page's notifications row read it; the shell
//! tells it whether the daemon may type, and asks it what toast a dictation
//! that went to the clipboard shows.

use gpui_kit::{Context, Task};
use super_voice_app_core::pages::setup::{
    self as page, Effect, Input, Pane, READY_FOR, SetupState,
};

use crate::effects;

/// The setup, as a GPUI model.
pub struct Setup {
    pub state: SetupState,
    /// This is the app's first launch on this account.
    first_launch: bool,
    /// Does nothing but keep `state` as it is: for a screenshot.
    fixed: bool,
    /// Sends `Folded` once the Ready card has shown long enough.
    fold: Option<Task<()>>,
    /// Asks macOS whether the app may type: what an `Open` that follows
    /// waits for.
    asking: Option<Task<()>>,
}

impl Setup {
    /// The setup of this Mac and this install.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: SetupState::new(Pane::here()),
            first_launch: page::first_launch(&super_voice_shared::paths::state_dir()),
            fixed: false,
            fold: None,
            asking: None,
        }
    }

    /// A setup that stays as `state` and does nothing: for a screenshot.
    #[cfg(feature = "screenshot")]
    #[must_use]
    pub fn fixed(state: SetupState) -> Self {
        Self {
            state,
            first_launch: false,
            fixed: true,
            fold: None,
            asking: None,
        }
    }

    /// Whether this is the app's first launch.
    #[must_use]
    pub const fn first_launch(&self) -> bool {
        self.first_launch
    }

    /// Take in `input`: the core decides, this does what it says.
    pub fn send(&mut self, input: Input, cx: &mut Context<Self>) {
        let effects = page::update(&mut self.state, input);
        if !self.fixed {
            for effect in effects {
                self.effect(effect, cx);
            }
        }
        cx.notify();
    }

    fn effect(&mut self, effect: Effect, cx: &mut Context<Self>) {
        match effect {
            Effect::AskToType => {
                let asking = cx.background_executor().spawn(async { ask_to_type() });
                self.asking = Some(asking);
            }
            // Once macOS was asked, so the pane lists Super Voice when it
            // opens.
            Effect::Open(url) => match self.asking.take() {
                Some(asking) => cx
                    .spawn(async move |this, cx| {
                        asking.await;
                        _ = this.update(cx, |_, cx| cx.open_url(&url));
                    })
                    .detach(),
                None => cx.open_url(&url),
            },
            Effect::FoldLater => self.fold = Some(Self::later(READY_FOR, Input::Folded, cx)),
            Effect::ReadNotifications => {
                cx.spawn(async move |this, cx| {
                    let on = cx
                        .background_executor()
                        .spawn(async { notifications_allowed() })
                        .await;
                    _ = this.update(cx, |setup, cx| {
                        setup.send(Input::NotificationsRead(on), cx);
                    });
                })
                .detach();
            }
            Effect::ReadMethod => {
                effects::spawn(page::run_read_method(), cx, |setup, input, cx| {
                    if let Some(input) = input {
                        setup.send(input, cx);
                    }
                })
                .detach();
            }
        }
    }

    /// Send `input` after `wait`.
    fn later(wait: std::time::Duration, input: Input, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            _ = this.update(cx, |setup, cx| setup.send(input, cx));
        })
    }
}

/// Ask macOS whether this app may type into other apps, letting it prompt
/// and list Super Voice in the pane. Blocking: it waits for macOS. Nothing
/// to ask elsewhere.
fn ask_to_type() {
    #[cfg(target_os = "macos")]
    crate::macos::ask_to_type();
}

/// Whether macOS shows this app's notifications, which are the daemon's
/// banners too: `None` where the app can't tell.
fn notifications_allowed() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::notifications_allowed()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
