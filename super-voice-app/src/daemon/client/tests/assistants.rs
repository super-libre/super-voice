// SPDX-License-Identifier: GPL-3.0-only
//! `v1/assistants` — the Assistants page's eight calls.

use serde_json::json;
use super_voice_shared::models::assistants::{AssistantId, AssistantState};

use super::fake_daemon::{FakeDaemon, Reply};
use crate::daemon::client::v1::assistants::{
    connect_assistant, create_key, disconnect_assistant, list_assistants, remove_key,
    set_announce_when_needed, set_conversation_context, set_show_talk,
};

/// What the daemon answers the list's calls with: Claude Code in `claude`'s
/// state, and one key made by hand.
fn list(claude: &str, announced: bool) -> serde_json::Value {
    json!({
        "assistants": [
            { "id": "claude-code", "state": claude, "announce_when_needed": announced },
            { "id": "codex", "state": "found", "announce_when_needed": true },
            { "id": "opencode", "state": "not_found", "announce_when_needed": true },
            { "id": "antigravity", "state": "not_found", "announce_when_needed": true },
        ],
        "custom_keys": [
            { "id": "0f3c9a1e", "name": "My editor", "created_at": "2026-11-19T20:15:00Z" },
        ],
        "address": "http://127.0.0.1:7300/v1/mcp",
    })
}

/// Each call goes to its path with its method and body, and reads the answer.
#[tokio::test]
async fn each_call_reaches_its_route_and_reads_the_answer() {
    let daemon = FakeDaemon::start().await;
    daemon.reply(Reply::json(&list("found", true)));
    daemon.reply(Reply::json(&list("connected", true)));
    daemon.reply(Reply::json(&list("connected", false)));
    daemon.reply(Reply::json(&list("connected", false)));
    daemon.reply(Reply::json(&list("connected", false)));
    daemon.reply(Reply::json(&json!({
        "id": "7a2b",
        "name": "Zed",
        "key": "sva_0123",
        "address": "http://127.0.0.1:7300/v1/mcp",
    })));
    daemon.reply(Reply::json(&list("connected", false)));
    daemon.reply(Reply::json(&list("found", false)));

    let listed = list_assistants().await.expect("listed");
    assert_eq!(listed.assistants[0].id, AssistantId::ClaudeCode);
    assert_eq!(listed.assistants[0].state, AssistantState::Found);
    assert_eq!(listed.assistants[2].state, AssistantState::NotFound);
    assert_eq!(listed.custom_keys[0].name, "My editor");
    assert_eq!(listed.address, "http://127.0.0.1:7300/v1/mcp");

    let connected = connect_assistant(AssistantId::ClaudeCode)
        .await
        .expect("connected");
    assert_eq!(connected.assistants[0].state, AssistantState::Connected);

    let quiet = set_announce_when_needed(AssistantId::ClaudeCode, false)
        .await
        .expect("set");
    assert!(!quiet.assistants[0].announce_when_needed);

    let heard = set_conversation_context(AssistantId::Codex, false)
        .await
        .expect("set");
    // A daemon that does not say uses the conversation.
    assert!(heard.assistants[1].conversation_context);

    let shown = set_show_talk(AssistantId::ClaudeCode, false)
        .await
        .expect("set");
    // A daemon that does not say shows Talk.
    assert!(shown.assistants[0].show_talk);

    let key = create_key("Zed".to_string()).await.expect("made");
    assert_eq!(key.name, "Zed");
    assert_eq!(key.key, "sva_0123");

    remove_key("0f3c9a1e".to_string()).await.expect("removed");

    let disconnected = disconnect_assistant(AssistantId::ClaudeCode)
        .await
        .expect("disconnected");
    assert_eq!(disconnected.assistants[0].state, AssistantState::Found);

    let sent: Vec<(String, String, serde_json::Value)> = daemon
        .requests()
        .iter()
        .map(|r| (r.method.clone(), r.path().to_string(), r.json()))
        .collect();
    assert_eq!(
        sent,
        [
            ("GET".to_string(), "/assistants".to_string(), json!(null)),
            (
                "POST".to_string(),
                "/assistants/claude-code/connect".to_string(),
                json!({}),
            ),
            (
                "PATCH".to_string(),
                "/assistants/claude-code".to_string(),
                json!({ "announce_when_needed": false }),
            ),
            (
                "PATCH".to_string(),
                "/assistants/codex".to_string(),
                json!({ "conversation_context": false }),
            ),
            (
                "PATCH".to_string(),
                "/assistants/claude-code".to_string(),
                json!({ "show_talk": false }),
            ),
            (
                "POST".to_string(),
                "/assistants/keys".to_string(),
                json!({ "name": "Zed" }),
            ),
            (
                "DELETE".to_string(),
                "/assistants/keys/0f3c9a1e".to_string(),
                json!(null),
            ),
            (
                "DELETE".to_string(),
                "/assistants/claude-code".to_string(),
                json!(null),
            ),
        ]
    );
}

/// The daemon's refusal reaches the page as its own words.
#[tokio::test]
async fn a_refused_connect_reports_the_daemons_reason() {
    let daemon = FakeDaemon::start().await;
    daemon.reply(Reply::status(
        409,
        &json!({
            "status": "error",
            "error_code": "assistant_not_found",
            "message": "assistant_not_found",
        }),
    ));

    let error = connect_assistant(AssistantId::Antigravity)
        .await
        .expect_err("refused");

    assert!(error.to_string().contains("assistant_not_found"), "{error}");
    assert_eq!(daemon.request().path(), "/assistants/antigravity/connect");
}
