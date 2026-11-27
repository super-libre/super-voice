// SPDX-License-Identifier: GPL-3.0-only
//! Daemon-facing operations for the app.
//!
//! Mirrors the daemon's server `v1/` tree: one module per endpoint
//! family under [`v1`], built on the shared Unix-socket transport, which
//! [`internal::transport`] wraps to say each request in the log, and the
//! cached `settings`-scope token ([`internal::session`]).

#[cfg(test)]
mod path_contract;
#[cfg(test)]
mod tests;

pub(crate) mod internal;
// Every call under `v1` fails the same way: the daemon could not be reached,
// refused the settings token, or answered with an error. `HttpError` says
// which, so no wrapper repeats it in an `# Errors` section.
#[allow(
    clippy::missing_errors_doc,
    reason = "HttpError says how each call fails"
)]
pub mod v1;

pub use v1::backends::list_backends;
pub use v1::backends::options::{clear_backend_option, set_backend_option};
pub use v1::backends::secrets::{clear_backend_secret, list_backend_secrets, set_backend_secret};
// Only the two types cross a module boundary — the Contexts page's messages
// and state name them. Every call goes through `v1::contexts` directly, the way
// the handler that makes it does.
pub(crate) use internal::session::app_name;
pub use internal::session::{SETTINGS_SCOPES, app_id, set_identity, use_scene_token};
pub use internal::transport::quietly;
pub use v1::contexts::backend::{BackendContext, ContextMode};
pub use v1::gpu_info::get_gpu_info;
pub use v1::ping::{ping_daemon, test_daemon_connection};
pub use v1::status::{get_can_type, get_status};
pub use v1::transcribe::{RecordEvent, record_command_stream, stop_record_command};

pub use v1::pipeline::backend::list_stage_backends;
pub use v1::pipeline::device::{
    get_model_device, list_model_devices, list_stage_devices, set_model_device,
};
pub use v1::pipeline::model::{
    cancel_download, get_download_status, get_stage_model, reload_stage_model, set_stage_model,
    unload_stage_model,
};
pub use v1::pipeline::stage::{clear_stage_backend, set_stage_backend};
pub use v1::pipeline::{ModelState, StageState, get_stage_view};

pub use v1::settings::audio_theme::{
    get_current_audio_theme, load_audio_themes, set_and_test_audio_theme, set_audio_theme,
};
pub use v1::settings::idle_unload::{get_idle_unload, set_idle_unload};
pub use v1::settings::notification_method::{get_notification_method, set_notification_method};
pub use v1::settings::preview_typing::{get_preview_typing, set_preview_typing};
pub use v1::settings::read_method::{get_read_method, set_read_method};
pub use v1::settings::recording_stop_mode::{get_recording_stop_mode, set_recording_stop_mode};
pub use v1::settings::update_beta_optin::set_update_beta_optin;
pub use v1::settings::update_check_enabled::{get_update_check_enabled, set_update_check_enabled};
pub use v1::settings::volume::{get_volume, set_volume};
pub use v1::settings::write_method::{get_write_method, set_write_method, test_write_method};

pub use v1::update::{check_update_now, get_update_status};
