// SPDX-License-Identifier: GPL-3.0-only

//! Provides localization support for this crate, over the core's catalogs.

use i18n_embed::unic_langid::LanguageIdentifier;

/// The catalogs and their loader are the core's, which every app shares.
pub use super_voice_app_core::i18n::LANGUAGE_LOADER;

/// Applies the requested language(s) to requested translations from the `fl!()` macro.
pub fn init(requested_languages: &[LanguageIdentifier]) {
    super_voice_app_core::i18n::select(requested_languages);
}

/// Request a localized string by ID from the core's `i18n/` catalogs.
#[macro_export]
macro_rules! fl {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $message_id)
    }};

    ($message_id:literal, $($args:expr),*) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $message_id, $($args), *)
    }};
}

/// Localized display names stay in the UI; the shared enums keep their wire values.
pub fn recording_stop_mode_name(
    mode: super_voice_shared::models::recording_stop_mode::RecordingStopMode,
) -> String {
    use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
    match mode {
        RecordingStopMode::SilenceOnly => crate::fl!("stop-silence"),
        RecordingStopMode::SilenceAndManual => crate::fl!("stop-silence-manual"),
        RecordingStopMode::ManualOnly => crate::fl!("stop-manual"),
    }
}

pub fn read_method_name(method: super_voice_shared::models::read_method::ReadMethod) -> String {
    use super_voice_shared::models::read_method::ReadMethod;
    match method {
        ReadMethod::Selection => crate::fl!("read-method-selection"),
        ReadMethod::Clipboard => crate::fl!("read-method-clipboard"),
    }
}

pub fn notification_method_name(
    method: super_voice_shared::models::notification_method::NotificationMethod,
) -> String {
    use super_voice_shared::models::notification_method::NotificationMethod;
    match method {
        NotificationMethod::Auto => crate::fl!("auto-recommended"),
        NotificationMethod::Desktop => crate::fl!("desktop-notification"),
        NotificationMethod::Typed => crate::fl!("type-into-window"),
        NotificationMethod::Off => crate::fl!("off"),
    }
}

pub fn write_method_name(method: super_voice_shared::models::write_method::WriteMethod) -> String {
    use super_voice_shared::models::write_method::WriteMethod;
    match method {
        WriteMethod::Auto => crate::fl!("auto-recommended"),
        WriteMethod::XdgDesktopPortal => crate::fl!("xdg-desktop-portal"),
        WriteMethod::Ydotool => "ydotool".to_string(),
        WriteMethod::BuiltIn => crate::fl!("built-in"),
    }
}

pub fn audio_theme_name(theme: super_voice_shared::models::theme::AudioTheme) -> String {
    use super_voice_shared::models::theme::AudioTheme;
    match theme {
        AudioTheme::Classic => crate::fl!("theme-classic"),
        AudioTheme::Gentle => crate::fl!("theme-gentle"),
        AudioTheme::Minimal => crate::fl!("theme-minimal"),
        AudioTheme::SciFi => crate::fl!("theme-scifi"),
        AudioTheme::Musical => crate::fl!("theme-musical"),
        AudioTheme::Nature => crate::fl!("theme-nature"),
        AudioTheme::Retro => crate::fl!("theme-retro"),
        AudioTheme::Silent => crate::fl!("theme-silent"),
    }
}

/// `text` without the Unicode isolation marks Fluent puts around each
/// variable it fills in, for a test that compares a message with plain text.
#[cfg(test)]
pub fn without_isolation(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}
