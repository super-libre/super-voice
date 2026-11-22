// SPDX-License-Identifier: GPL-3.0-only
pub mod assistants;
pub mod audio;
pub mod backend_headers;
mod backend_supports;
pub mod cli;
pub mod config;
pub mod crash;
pub mod daemon;
pub mod download_progress;
pub use super_engine_daemon::download_stream;
pub mod input;
pub mod limits;
pub mod old_data;
pub mod output;
#[cfg(target_os = "linux")]
mod portal;
pub mod registry;
pub mod secret_headers;
pub use super_engine_daemon::resource_management;
pub mod self_update;
pub mod services;
pub mod shortcuts;
pub mod stt_models;
pub mod tts;

// Re-export the main run function, and the status it exits with when another
// daemon already serves its socket.
pub use daemon_main::{EXIT_ALREADY_SERVED, run};

mod daemon_main;
mod num_cast;

/// Re-export the shared rustls installer from `super-engine-forge`, so
/// `main` and the tests install the provider through one implementation that
/// lives beside the reqwest client factory.
pub use super_engine_forge::install_crypto_provider;
