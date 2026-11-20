// SPDX-License-Identifier: GPL-3.0-only
//! `/v1/assistants` end to end: a real daemon, in a home of its own,
//! connecting assistants by writing their configs under that home, and
//! running a fake `claude` that only notes what it was asked.
//!
//! Every assistant's config is under the test's home: `HOME` is the test's,
//! and the variables that would point an assistant elsewhere are removed. The
//! programs the daemon finds are the fakes on the test's own `PATH`, which
//! comes first. Nothing here reaches the user's own assistants.
//!
//! The routes that make or show a key are for Super Voice's own clients
//! alone. The test binary is not one, so the daemon is told to count it as
//! one, with `SUPER_VOICE_TREAT_AS_OFFICIAL`, except where the refusal is
//! what is tested.

mod common;

use common::{Method, StatusCode, TestDaemon};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use super_voice_shared::daemon::http_client;

/// The variables that would point an assistant's config out of the test's
/// home.
const ELSEWHERE: [&str; 3] = ["CODEX_HOME", "GEMINI_CLI_HOME", "CLAUDE_CONFIG_DIR"];

/// A fake `claude`: notes each command in `$HOME/claude-calls`, and installs
/// and uninstalls the plugin in the settings Claude Code keeps it in.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "$*" >> "$HOME/claude-calls"
case "$*" in
  "plugin install super-voice@super-voice --scope user")
    mkdir -p "$HOME/.claude"
    printf '{"enabledPlugins": {"super-voice@super-voice": true}}' > "$HOME/.claude/settings.json" ;;
  "plugin uninstall super-voice@super-voice --scope user")
    printf '{"enabledPlugins": {}}' > "$HOME/.claude/settings.json" ;;
esac
"#;

/// The variable that names where Claude Code's plugin comes from.
const MARKETPLACE_VAR: &str = "SUPER_VOICE_CLAUDE_MARKETPLACE";

/// A daemon whose home is `home`, finding the programs in `home/bin`, and
/// a token for it with the `settings` scope.
async fn start(label: &str, official: bool) -> (TestDaemon, String, PathBuf) {
    start_with(label, official, &[]).await
}

/// [`start`], with `env` set for the daemon as well.
async fn start_with(
    label: &str,
    official: bool,
    env: &[(&str, &str)],
) -> (TestDaemon, String, PathBuf) {
    let mut builder = common::daemon(label);
    let home = builder.home().root().join("home");
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir bin");
    write_program(&bin.join("claude"), FAKE_CLAUDE);
    write_program(&bin.join("codex"), "#!/bin/sh\nexit 0\n");
    builder = builder
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        // No desktop notification reaches whoever runs the suite.
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus");
    for var in ELSEWHERE {
        builder = builder.without(var);
    }
    // The plugin comes from where the test says, not the shell running it.
    builder = builder.without(MARKETPLACE_VAR);
    for (key, value) in env {
        builder = builder.env(key, value);
    }
    if official {
        builder = builder.env("SUPER_VOICE_TREAT_AS_OFFICIAL", "1");
    }
    let daemon = builder.start().await;
    let token = http_client::auth_request(
        daemon.socket().to_path_buf(),
        "assistants test",
        &["settings"],
    )
    .await
    .expect("auth_request")
    .session_token;
    (daemon, token, home)
}

fn write_program(path: &Path, script: &str) {
    std::fs::write(path, script).expect("write program");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod program");
    }
}

async fn call(
    daemon: &TestDaemon,
    token: &str,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    common::request(daemon.socket(), method, path, Some(token), body).await
}

/// Each assistant's `state`, by id.
fn states(list: &Value) -> Vec<(String, String)> {
    list["assistants"]
        .as_array()
        .expect("assistants")
        .iter()
        .map(|a| {
            (
                a["id"].as_str().expect("id").to_string(),
                a["state"].as_str().expect("state").to_string(),
            )
        })
        .collect()
}

fn state_of(list: &Value, id: &str) -> String {
    states(list)
        .into_iter()
        .find(|(each, _)| each == id)
        .map(|(_, state)| state)
        .unwrap_or_else(|| panic!("{id} is not listed: {list}"))
}

/// `found` when this machine has `program` where the daemon also looks
/// outside the home and `PATH`, `not_found` otherwise. Those directories are
/// the machine's own, not the test's.
fn on_this_machine(program: &str) -> &'static str {
    let found = [
        "/home/linuxbrew/.linuxbrew/bin",
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
    ]
    .iter()
    .any(|dir| Path::new(dir).join(program).is_file());
    if found { "found" } else { "not_found" }
}

/// Connecting Codex writes its entry and makes its key; the key is shown to
/// the CLI's route; disconnecting takes the entry out and revokes the key.
#[tokio::test]
async fn codex_is_connected_and_disconnected() {
    let (daemon, token, home) = start("assistants-codex", true).await;
    let config = home.join(".codex/config.toml");

    let (status, list) = call(&daemon, &token, Method::GET, "/assistants", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(state_of(&list, "claude-code"), "found");
    assert_eq!(state_of(&list, "codex"), "found");
    assert_eq!(state_of(&list, "opencode"), on_this_machine("opencode"));
    assert_eq!(state_of(&list, "gemini"), on_this_machine("gemini"));
    assert_eq!(list["custom_keys"], json!([]));
    assert!(
        list["address"]
            .as_str()
            .is_some_and(|a| a.starts_with("http://127.0.0.1:") && a.ends_with("/v1/mcp")),
        "{list}"
    );

    let (status, list) = call(
        &daemon,
        &token,
        Method::POST,
        "/assistants/codex/connect",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(state_of(&list, "codex"), "connected");
    let written = std::fs::read_to_string(&config).expect("Codex's config was written");
    assert!(written.contains("[mcp_servers.super-voice]"), "{written}");
    assert!(written.contains("assistant-header codex"), "{written}");
    assert!(
        !written.contains("sva_"),
        "Codex's config holds no key: {written}"
    );

    let (status, body) = call(&daemon, &token, Method::GET, "/assistants/codex/key", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let key = body["key"].as_str().expect("key").to_string();
    assert!(key.starts_with("sva_"), "{body}");

    // Connecting again keeps the key it has.
    let (status, _) = call(
        &daemon,
        &token,
        Method::POST,
        "/assistants/codex/connect",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, again) = call(&daemon, &token, Method::GET, "/assistants/codex/key", None).await;
    assert_eq!(again["key"], key.as_str());

    let (status, body) = call(
        &daemon,
        &token,
        Method::GET,
        "/assistants/opencode/key",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "not_connected");

    let (status, list) = call(&daemon, &token, Method::DELETE, "/assistants/codex", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(state_of(&list, "codex"), "found");
    let left = std::fs::read_to_string(&config).expect("config");
    assert!(!left.contains("super-voice"), "{left}");
    let (status, body) = call(&daemon, &token, Method::GET, "/assistants/codex/key", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the key is revoked: {body}");

    let (status, _) = call(&daemon, &token, Method::DELETE, "/assistants/codex", None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "disconnecting again changes nothing"
    );
}

/// Connecting Claude Code installs the plugin with the `claude` it found,
/// after adding its marketplace; disconnecting uninstalls it.
#[tokio::test]
async fn claude_code_gets_the_plugin_installed() {
    let (daemon, token, home) = start("assistants-claude", true).await;
    let calls = home.join("claude-calls");

    let (status, list) = call(
        &daemon,
        &token,
        Method::POST,
        "/assistants/claude-code/connect",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(state_of(&list, "claude-code"), "connected");
    let ran = std::fs::read_to_string(&calls).expect("claude ran");
    assert_eq!(
        ran.lines().collect::<Vec<_>>(),
        [
            "plugin marketplace add super-libre/super-voice --scope user",
            "plugin install super-voice@super-voice --scope user",
        ]
    );
    let (status, body) = call(
        &daemon,
        &token,
        Method::GET,
        "/assistants/claude-code/key",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, list) = call(
        &daemon,
        &token,
        Method::DELETE,
        "/assistants/claude-code",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(state_of(&list, "claude-code"), "found");
    let ran = std::fs::read_to_string(&calls).expect("claude ran");
    assert_eq!(
        ran.lines().last(),
        Some("plugin uninstall super-voice@super-voice --scope user")
    );
    let (status, _) = call(
        &daemon,
        &token,
        Method::GET,
        "/assistants/claude-code/key",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the key is revoked");
}

/// With `SUPER_VOICE_CLAUDE_MARKETPLACE` set, Connect adds that marketplace,
/// like a checkout's directory, in place of the one on GitHub.
#[tokio::test]
async fn the_variable_names_where_the_plugin_comes_from() {
    let (daemon, token, home) = start_with(
        "assistants-claude-marketplace",
        true,
        &[(MARKETPLACE_VAR, "/src/super-voice")],
    )
    .await;

    let (status, list) = call(
        &daemon,
        &token,
        Method::POST,
        "/assistants/claude-code/connect",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ran = std::fs::read_to_string(home.join("claude-calls")).expect("claude ran");
    assert_eq!(
        ran.lines().collect::<Vec<_>>(),
        [
            "plugin marketplace add /src/super-voice --scope user",
            "plugin install super-voice@super-voice --scope user",
        ]
    );
}

/// Asking aloud is kept per assistant.
#[tokio::test]
async fn each_assistant_asks_aloud_on_its_own() {
    let (daemon, token, _home) = start("assistants-aloud", true).await;
    let aloud = |list: &Value, id: &str| {
        list["assistants"]
            .as_array()
            .expect("assistants")
            .iter()
            .find(|a| a["id"] == id)
            .map(|a| a["ask_questions_aloud"].clone())
    };

    let (status, list) = call(
        &daemon,
        &token,
        Method::PATCH,
        "/assistants/codex",
        Some(&json!({ "ask_questions_aloud": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(aloud(&list, "codex"), Some(json!(true)));
    assert_eq!(aloud(&list, "claude-code"), Some(json!(false)));

    let (status, body) = call(
        &daemon,
        &token,
        Method::PATCH,
        "/assistants/codex",
        Some(&json!({ "ask_questions_aloud": "yes" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = call(
        &daemon,
        &token,
        Method::PATCH,
        "/assistants/robot",
        Some(&json!({ "ask_questions_aloud": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "unknown_assistant");
}

/// A key made by hand is shown once, listed by its name, and revoked; an
/// assistant's name is not one to give it.
#[tokio::test]
async fn keys_made_by_hand_are_listed_and_revoked() {
    let (daemon, token, _home) = start("assistants-keys", true).await;

    let (status, made) = call(
        &daemon,
        &token,
        Method::POST,
        "/assistants/keys",
        Some(&json!({ "name": "  My editor " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{made}");
    assert_eq!(made["name"], "My editor");
    assert!(
        made["key"].as_str().is_some_and(|k| k.starts_with("sva_")),
        "{made}"
    );
    assert!(
        made["address"]
            .as_str()
            .is_some_and(|a| a.ends_with("/v1/mcp")),
        "{made}"
    );
    let id = made["id"].as_str().expect("id").to_string();

    let (_, list) = call(&daemon, &token, Method::GET, "/assistants", None).await;
    let keys = list["custom_keys"].as_array().expect("custom_keys");
    assert_eq!(keys.len(), 1, "{list}");
    assert_eq!(keys[0]["id"], id.as_str());
    assert_eq!(keys[0]["name"], "My editor");
    assert!(
        keys[0].get("key").is_none(),
        "the list shows no key: {list}"
    );

    for (name, refusal) in [("codex", "name_reserved"), ("   ", "name_required")] {
        let (status, body) = call(
            &daemon,
            &token,
            Method::POST,
            "/assistants/keys",
            Some(&json!({ "name": name })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name:?}: {body}");
        assert_eq!(body["message"], refusal);
    }

    let (status, list) = call(
        &daemon,
        &token,
        Method::DELETE,
        &format!("/assistants/keys/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["custom_keys"], json!([]));
    let (status, body) = call(
        &daemon,
        &token,
        Method::DELETE,
        &format!("/assistants/keys/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "unknown_key");
}

/// An assistant the daemon finds no sign of is not connected.
#[tokio::test]
async fn an_assistant_that_is_not_here_is_not_connected() {
    if on_this_machine("gemini") == "found" {
        eprintln!("skipped: this machine has gemini where the daemon looks for it");
        return;
    }
    let (daemon, token, home) = start("assistants-missing", true).await;
    let (status, body) = call(
        &daemon,
        &token,
        Method::POST,
        "/assistants/gemini/connect",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error_code"], "assistant_not_found");
    assert!(!home.join(".gemini").exists(), "nothing was written");
    let (_, list) = call(&daemon, &token, Method::GET, "/assistants", None).await;
    assert_eq!(state_of(&list, "gemini"), "not_found");
}

/// Only Super Voice's own clients may make a key or see one. Listing,
/// disconnecting and changing are for any `settings` token.
#[tokio::test]
async fn only_super_voice_s_own_clients_make_or_see_a_key() {
    let (daemon, token, home) = start("assistants-unofficial", false).await;
    for (method, path, body) in [
        (Method::POST, "/assistants/codex/connect", None),
        (Method::GET, "/assistants/codex/key", None),
        (
            Method::POST,
            "/assistants/keys",
            Some(json!({ "name": "Mine" })),
        ),
    ] {
        let (status, refused) = call(&daemon, &token, method.clone(), path, body.as_ref()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {refused}");
        assert_eq!(
            refused["data"]["reason"], "not_official_client",
            "{refused}"
        );
    }
    assert!(!home.join(".codex").exists(), "nothing was written");

    let (status, list) = call(&daemon, &token, Method::GET, "/assistants", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let (status, _) = call(&daemon, &token, Method::DELETE, "/assistants/codex", None).await;
    assert_eq!(status, StatusCode::OK);
}
