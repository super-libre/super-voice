// SPDX-License-Identifier: GPL-3.0-only
//! Toasts for the core's notices: each page's `Outcome` carries a `notice` to
//! show and `clear_notice` to take the last one down. A page has one toast at
//! a time, keyed on the page's type: a new notice replaces the old in place.
//!
//! ```ignore
//! toast::notice::<Self>(window, cx, outcome.clear_notice, outcome.notice.map(|n| Toast::error(words(n))));
//! ```
//!
//! An error that says the daemon can't be reached is not shown while the
//! Connection page says so, and goes when the page comes up: the page's
//! words are enough.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::{App, SharedString, Window};
use super_voice_app_core::pages::connection::says_unreachable;

/// What a toast says, and what it offers.
#[derive(Clone)]
pub struct Toast {
    kind: NotificationType,
    title: Option<SharedString>,
    message: SharedString,
    action: Option<(SharedString, ToastAction)>,
}

type ToastAction = Rc<dyn Fn(&mut Window, &mut App)>;

impl Toast {
    /// Something that happened: an update is ready.
    #[must_use]
    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::new(NotificationType::Info, message)
    }

    /// Something that worked: copied.
    #[must_use]
    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new(NotificationType::Success, message)
    }

    /// Something to look at, that didn't fail.
    #[must_use]
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(NotificationType::Warning, message)
    }

    /// Something that failed: couldn't save, couldn't read.
    #[must_use]
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(NotificationType::Error, message)
    }

    fn new(kind: NotificationType, message: impl Into<SharedString>) -> Self {
        Self {
            kind,
            title: None,
            message: message.into(),
            action: None,
        }
    }

    /// A bold line above the message.
    #[must_use]
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// A button in the toast, which closes it when pressed. A toast with one
    /// stays until it is closed.
    #[must_use]
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        on_press: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(on_press)));
        self
    }

    /// Whether it is an error that says the daemon can't be reached.
    fn says_unreachable(&self) -> bool {
        matches!(self.kind, NotificationType::Error) && says_unreachable(&self.message)
    }

    /// The kit's notification, keyed on `P`; one that says the daemon can't
    /// be reached is keyed on [`Unreachable`] too, so they can all go at once.
    fn notification<P: 'static>(self) -> Notification {
        let note = Notification::new()
            .with_type(self.kind)
            .message(self.message.clone());
        let mut note = if self.says_unreachable() {
            note.id1::<Unreachable>(std::any::type_name::<P>())
        } else {
            note.id::<P>()
        };
        if let Some(title) = self.title {
            note = note.title(title);
        }
        if let Some((label, on_press)) = self.action {
            note = note.action(move |_, _, _| {
                let on_press = on_press.clone();
                Button::new("toast-action")
                    .small()
                    .label(label.clone())
                    .on_click(move |_, window, cx| on_press(window, cx))
            });
        }
        note
    }
}

/// What the toasts that say the daemon can't be reached are keyed on, besides
/// their page.
pub struct Unreachable;

/// Show `toast` as page `P`'s, replacing the one it showed before. One that
/// says the daemon can't be reached is dropped while the Connection page
/// says so.
///
/// `toast::show::<Self>(window, cx, Toast::success(t("about-copied")));`
pub fn show<P: 'static>(window: &mut Window, cx: &mut App, toast: Toast) {
    // One toast a page: this one replaces the one before in place, whichever
    // key that one had.
    if toast.says_unreachable() {
        if crate::connection::is_down(cx) {
            return;
        }
        window.remove_notification::<P>(cx);
    } else {
        window.remove_notification1::<Unreachable>(std::any::type_name::<P>(), cx);
    }
    window.push_notification(toast.notification::<P>(), cx);
}

/// Take page `P`'s toast down.
pub fn clear<P: 'static>(window: &mut Window, cx: &mut App) {
    window.remove_notification::<P>(cx);
    window.remove_notification1::<Unreachable>(std::any::type_name::<P>(), cx);
}

/// Take down every toast that says the daemon can't be reached: the
/// Connection page says it now.
pub fn drop_unreachable(window: &mut Window, cx: &mut App) {
    window.remove_notification::<Unreachable>(cx);
}

/// Do what a core `Outcome` says of page `P`'s toast: take it down when
/// `clear_notice`, then show `notice`'s.
pub fn notice<P: 'static>(
    window: &mut Window,
    cx: &mut App,
    clear_notice: bool,
    notice: Option<Toast>,
) {
    if clear_notice {
        clear::<P>(window, cx);
    }
    if let Some(toast) = notice {
        show::<P>(window, cx, toast);
    }
}
