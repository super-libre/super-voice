// SPDX-License-Identifier: GPL-3.0-only
//! Hot-patching for development (`just app-watch`), with the `hotpatch`
//! feature: Dioxus's subsecond, served by its CLI (`dx serve --hot-patch`).
//!
//! On every save, `dx` compiles only what changed and patches it into the
//! running app; the app draws its windows again and the next frame runs the
//! new code. What a patch can change is the code of functions: a view's
//! `render`, what it calls, the words. A change to a struct's fields, to
//! `main` or to what runs once at start needs a rebuild, which `dx` does.
//!
//! Without the feature, [`call`] is a plain call and [`connect`] does nothing.

use gpui_kit::App;

/// Run `f` through subsecond's jump table, so a patch reaches it.
pub fn call<O>(f: impl FnMut() -> O) -> O {
    #[cfg(feature = "hotpatch")]
    {
        subsecond::call(f)
    }
    #[cfg(not(feature = "hotpatch"))]
    {
        let mut f = f;
        f()
    }
}

/// Take patches from `dx serve`, and draw every window again after each.
pub fn connect(cx: &mut App) {
    #[cfg(feature = "hotpatch")]
    {
        use std::sync::Arc;

        dioxus_devtools::connect_subsecond();
        // A tokio primitive needs no tokio runtime to wait on, so the UI
        // thread's executor waits on it.
        let patched = Arc::new(tokio::sync::Notify::new());
        subsecond::register_handler(Arc::new({
            let patched = patched.clone();
            move || patched.notify_one()
        }));
        cx.spawn(async move |cx| {
            loop {
                patched.notified().await;
                log::info!("patched; drawing again");
                cx.update(App::refresh_windows);
            }
        })
        .detach();
    }
    #[cfg(not(feature = "hotpatch"))]
    let _ = cx;
}
