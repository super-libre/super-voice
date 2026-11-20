// SPDX-License-Identifier: GPL-3.0-only
//! `voice assistant-header <assistant>` end to end: what an assistant that
//! gets its key by running a command, such as Codex, reads on stdout.
//!
//! The daemon runs in a home of its own, with `HOME` the test's, so Codex's
//! config is written there, and it counts the test as one of Super Voice's
//! own clients (`SUPER_VOICE_TREAT_AS_OFFICIAL`). The CLI keeps its token in
//! the daemon's home.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use super_engine_test_daemon::{Method, StatusCode, TestDaemon};
use super_voice_shared::product::SUPER_VOICE;

const CLI_BIN: &str = env!("CARGO_BIN_EXE_super-voice-cli");

/// The daemon binary, in the same target directory as the CLI's.
fn daemon_bin() -> PathBuf {
    PathBuf::from(CLI_BIN)
        .parent()
        .expect("cli bin parent dir")
        .join("super-voice-daemon")
}

/// A daemon whose home has Codex's config directory, so it finds Codex.
async fn start() -> TestDaemon {
    let builder = TestDaemon::build(&SUPER_VOICE, daemon_bin(), "cli-assistant-header");
    let home = builder.home().root().join("home");
    std::fs::create_dir_all(home.join(".codex")).expect("mkdir .codex");
    builder
        .env("HOME", &home)
        .without("CODEX_HOME")
        .env("SUPER_VOICE_TREAT_AS_OFFICIAL", "1")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
        .start()
        .await
}

/// Run `voice assistant-header <assistant>` against `socket`. Returns
/// `(exit code, stdout, stderr)`.
fn header(daemon: &TestDaemon, socket: &str, assistant: &str) -> (i32, String, String) {
    let out = Command::new(CLI_BIN)
        .env("XDG_STATE_HOME", &daemon.home().state)
        .env("SUPER_VOICE_AUTO_APPROVE", "1")
        .args(["--socket", socket, "assistant-header", assistant])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn prints_the_key_of_a_connected_assistant_and_nothing_else() {
    let daemon = start().await;
    let socket = daemon.socket().to_str().expect("socket utf8").to_string();

    let (code, stdout, stderr) = tokio::task::block_in_place(|| header(&daemon, &socket, "codex"));
    assert_eq!(
        code, 1,
        "not connected yet; stdout=`{stdout}` stderr=`{stderr}`"
    );
    assert!(stdout.is_empty(), "stdout=`{stdout}`");
    assert!(
        stderr.contains("has not connected Codex"),
        "stderr=`{stderr}`"
    );

    let token = super_voice_shared::daemon::http_client::auth_request(
        daemon.socket().to_path_buf(),
        "assistant header test",
        &["settings"],
    )
    .await
    .expect("auth_request")
    .session_token;
    let (status, list) = daemon
        .request(
            Method::POST,
            "/assistants/codex/connect",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let (_, body) = daemon
        .request(Method::GET, "/assistants/codex/key", Some(&token), None)
        .await;
    let key = body["key"].as_str().expect("key").to_string();

    let (code, stdout, stderr) = tokio::task::block_in_place(|| header(&daemon, &socket, "codex"));
    assert_eq!(code, 0, "stdout=`{stdout}` stderr=`{stderr}`");
    assert_eq!(stdout, format!("{{\"Authorization\":\"Bearer {key}\"}}\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn says_so_when_the_daemon_is_not_there() {
    let daemon = start().await;
    let gone = daemon.home().root().join("gone.sock");
    let (code, stdout, stderr) = tokio::task::block_in_place(|| {
        header(&daemon, gone.to_str().expect("utf8"), "claude-code")
    });
    assert_eq!(code, 1, "stdout=`{stdout}` stderr=`{stderr}`");
    assert!(stdout.is_empty(), "stdout=`{stdout}`");
    assert!(
        stderr.contains("Could not get Claude Code's key"),
        "stderr=`{stderr}`"
    );
}
