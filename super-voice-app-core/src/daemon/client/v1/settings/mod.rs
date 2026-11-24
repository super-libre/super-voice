// SPDX-License-Identifier: GPL-3.0-only
//! `/settings` — the daemon's stored preferences.
//!
//! Mirrors the daemon's `v1/settings/`. What lives here is decided by subject,
//! not by scope: the `settings` scope also guards [`super::backends`],
//! [`super::pipeline`] and [`super::registry`], and the app's token carries it
//! for all of them.
//!
//! Neighbours that are about settings without being one: [`super::gpu_info`],
//! [`super::models`], [`super::update`].

pub mod audio_theme;
pub mod idle_unload;
pub mod language;
pub mod notification_method;
pub mod preview_typing;
pub mod read_method;
pub mod recording_stop_mode;
pub mod update_beta_optin;
pub mod update_check_enabled;
pub mod volume;
pub mod write_method;
