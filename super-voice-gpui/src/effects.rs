// SPDX-License-Identifier: GPL-3.0-only
//! Running a core page's effects.
//!
//! A core page decides (`update(&mut state, input) -> Outcome`) and its
//! runners do the work, each answering with the page's next `Input`. A view
//! implements [`CorePage`] once: what each `Effect` is (a runner to start, or
//! something the UI does at once, such as copying), and how it takes in an
//! `Input`. Then [`run_effects`] starts every effect of an outcome on the
//! core's tokio runtime and hands each answer back to the view on the UI
//! thread, which takes it in like any press:
//!
//! ```ignore
//! fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
//!     let outcome = page::update(&mut self.state, input);
//!     effects::run_effects(self, outcome.effects, window, cx);
//!     cx.notify();
//! }
//! ```
//!
//! See `pages/about.rs` for a whole page. Models that have no window (the
//! connection, the readiness) use [`spawn`] instead.

use std::future::Future;
use std::pin::Pin;

use gpui_kit::{AsyncApp, Context, Task, Window};

use crate::runtime;

/// A core runner, ready to start: `runner(page::run_read(stage))`.
pub type Runner<I> = Pin<Box<dyn Future<Output = I> + Send + 'static>>;

/// Box `work` as a [`Runner`].
pub fn runner<I>(work: impl Future<Output = I> + Send + 'static) -> Runner<I> {
    Box::pin(work)
}

/// A view over one core page.
pub trait CorePage: Sized + 'static {
    /// The core page's `Input`.
    type Input: Send + 'static;
    /// The core page's `Effect`.
    type Effect;

    /// Take in `input`: call the core's `update`, then [`run_effects`] with
    /// its effects, show its notice, and notify.
    fn send(&mut self, input: Self::Input, window: &mut Window, cx: &mut Context<Self>);

    /// Do `effect`: at once for what the UI does itself (copy, open a link,
    /// open a dialog), returning `None`; or return the runner that does it,
    /// whose answer comes back through [`Self::send`].
    fn effect(
        &mut self,
        effect: Self::Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Self::Input>>;
}

/// Do every effect in `effects`: each runner on the core's runtime, its answer
/// back to `page` through [`CorePage::send`] on the UI thread.
///
/// The runs are detached: an answer that comes back after the page is gone is
/// dropped. A view that must cancel one keeps the [`Task`] from [`run`].
pub fn run_effects<P: CorePage>(
    page: &mut P,
    effects: impl IntoIterator<Item = P::Effect>,
    window: &mut Window,
    cx: &mut Context<P>,
) {
    for effect in effects {
        if let Some(work) = page.effect(effect, window, cx) {
            run(work, window, cx).detach();
        }
    }
}

/// Run one runner on the core's runtime, and hand its answer to the page.
/// Dropping the task stops waiting for it.
#[must_use = "dropping the task stops waiting for the answer; detach it to keep it"]
pub fn run<P: CorePage>(
    work: Runner<P::Input>,
    window: &mut Window,
    cx: &mut Context<P>,
) -> Task<()> {
    cx.spawn_in(window, async move |this, cx| {
        let input = match runtime::spawn(work).await {
            Ok(input) => input,
            Err(e) => {
                log::error!("a core runner stopped: {e}");
                return;
            }
        };
        if let Err(e) = this.update_in(cx, |page, window, cx| page.send(input, window, cx)) {
            log::debug!("the page is gone: {e}");
        }
    })
}

/// [`run`], for a model with no window: `answer` takes the runner's output.
#[must_use = "dropping the task stops waiting for the answer; detach it to keep it"]
pub fn spawn<T: 'static, I: Send + 'static>(
    work: impl Future<Output = I> + Send + 'static,
    cx: &mut Context<T>,
    answer: fn(&mut T, I, &mut Context<T>),
) -> Task<()> {
    cx.spawn(async move |this, cx: &mut AsyncApp| {
        let output = match runtime::spawn(work).await {
            Ok(output) => output,
            Err(e) => {
                log::error!("a core runner stopped: {e}");
                return;
            }
        };
        if let Err(e) = this.update(cx, |model, cx| answer(model, output, cx)) {
            log::debug!("the model is gone: {e}");
        }
    })
}
