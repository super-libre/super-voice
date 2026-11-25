// SPDX-License-Identifier: GPL-3.0-only
//! `super-voice-app`: the Super Voice app.

fn main() {
    // A banner for the daemon, posted before anything else starts: no window,
    // no GPUI. See `macos::notifier`.
    #[cfg(target_os = "macos")]
    if super_voice_gpui::macos::notifier::requested() {
        super_voice_gpui::macos::notifier::run();
    }

    super_voice_shared::logging::init();
    super_voice_gpui::run();
}
