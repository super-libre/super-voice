// SPDX-License-Identifier: GPL-3.0-only
//! `super-voice-mcp`: Super Voice as a Model Context Protocol server, so an AI
//! assistant such as Claude Code can talk to the user and hear them.
//!
//! A client of its own, apart from the `voice` command. The daemon lets its
//! own clients in without asking when they sit beside it, and the CLI is one
//! of them. This binary is not, so the user is asked before an assistant can
//! use the microphone or speak, sees this binary's path when asked, and can
//! revoke it alone.

mod server;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Arg, Command, value_parser};
use super_voice_shared::validation::get_http_socket_path;

fn main() -> Result<()> {
    // Logs go to stderr. Stdout carries the protocol and nothing else.
    super_voice_shared::logging::init();

    let matches = Command::new("super-voice-mcp")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Serve Super Voice to an AI assistant, such as Claude Code, over MCP on stdin and stdout")
        .arg(
            Arg::new("socket")
                .long("socket")
                .help("Path to the daemon HTTP Unix socket")
                .value_parser(value_parser!(PathBuf)),
        )
        .get_matches();
    let socket_path = matches
        .get_one::<PathBuf>("socket")
        .cloned()
        .unwrap_or_else(get_http_socket_path);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not start the async runtime")?
        .block_on(server::run(socket_path))
}

#[cfg(test)]
mod tests {
    /// The reason this is a binary of its own: the daemon asks the user before
    /// letting it in. On the list of Super Voice's own clients, it would be let
    /// in without asking, as the CLI is.
    #[test]
    fn the_daemon_asks_before_letting_it_in() {
        let clients = super_voice_shared::product::SUPER_VOICE.clients;
        assert!(!clients.contains(&env!("CARGO_BIN_NAME")), "{clients:?}");
    }
}
