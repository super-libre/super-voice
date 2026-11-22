// SPDX-License-Identifier: GPL-3.0-only
//! Which session a tool call comes from, by what its client says of itself.
//!
//! First what our own plugins say, then what each client puts in `_meta`.
//! A call that says nothing is named by its connection instead
//! ([`crate::assistants::sessions::Sessions::resolve`]):
//!
//! 1. The argument `session`, `{ "id", "agent", "folder" }`, which Super
//!    Voice's plugins add to every call. No tool declares it, so a model
//!    never fills it.
//! 2. Claude Code's `claudecode/sessionId` and `claudecode/agentId`.
//! 3. Codex's `sessionId`, the conversation's root thread, and `threadId`,
//!    an agent's own thread within it; the folder is the first git root in
//!    `x-codex-turn-metadata`'s `workspaces`.
//! 4. `OpenCode`'s `ai.opencode/sessionID`.
//!
//! A value the first source names is kept; a later one only fills what is
//! still missing.

use crate::assistants::sessions::Named;
use serde_json::Value;

/// The longest id, agent or folder kept. Longer is cut, so a client cannot
/// fill the daemon's memory with one name.
const MAX_LEN: usize = 512;

/// What `params`, a request's, say of the session it comes from.
pub(crate) fn identity(params: &Value) -> Named {
    let mut named = Named::default();
    let given = &params["arguments"]["session"];
    fill(
        &mut named,
        text(&given["id"]),
        text(&given["agent"]),
        text(&given["folder"]),
    );

    let meta = &params["_meta"];
    fill(
        &mut named,
        text(&meta["claudecode/sessionId"]),
        text(&meta["claudecode/agentId"]),
        None,
    );

    // Codex before 0.156 sends no `sessionId`; its turn metadata names the
    // conversation as `session_id`.
    let codex_root = text(&meta["sessionId"])
        .or_else(|| codex_turn(meta).and_then(|turn| text(&turn["session_id"])));
    let (codex_id, codex_agent) = match (codex_root, text(&meta["threadId"])) {
        (Some(root), Some(thread)) if root != thread => (Some(root), Some(thread)),
        (Some(root), _) => (Some(root), None),
        (None, thread) => (thread, None),
    };
    fill(&mut named, codex_id, codex_agent, codex_folder(meta));

    fill(&mut named, text(&meta["ai.opencode/sessionID"]), None, None);
    named
}

/// Fill what `named` is still missing.
fn fill(named: &mut Named, id: Option<String>, agent: Option<String>, folder: Option<String>) {
    if named.id.is_none() {
        named.id = id;
    }
    if named.agent.is_none() {
        named.agent = agent;
    }
    if named.folder.is_none() {
        named.folder = folder;
    }
}

/// Codex's `x-codex-turn-metadata`, which may come as an object or as its
/// JSON text.
fn codex_turn(meta: &Value) -> Option<Value> {
    match &meta["x-codex-turn-metadata"] {
        Value::String(json) => serde_json::from_str(json).ok(),
        Value::Object(_) => Some(meta["x-codex-turn-metadata"].clone()),
        _ => None,
    }
}

/// The first git root Codex names for the turn.
fn codex_folder(meta: &Value) -> Option<String> {
    codex_turn(meta)?["workspaces"]
        .as_object()?
        .keys()
        .find_map(|root| text(&Value::from(root.as_str())))
}

/// A non-blank string, cut to [`MAX_LEN`] characters.
fn text(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(MAX_LEN).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn named(id: Option<&str>, agent: Option<&str>, folder: Option<&str>) -> Named {
        Named {
            id: id.map(str::to_string),
            agent: agent.map(str::to_string),
            folder: folder.map(str::to_string),
        }
    }

    /// What our plugins add comes first, over what the client says itself.
    #[test]
    fn our_plugins_session_argument_comes_first() {
        let params = json!({
            "name": "speak",
            "arguments": {
                "text": "Hi.",
                "session": { "id": "cc-1", "agent": "sub-2", "folder": "/home/me/app" },
            },
            "_meta": { "claudecode/sessionId": "other" },
        });
        assert_eq!(
            identity(&params),
            named(Some("cc-1"), Some("sub-2"), Some("/home/me/app"))
        );
    }

    #[test]
    fn claude_codes_own_meta_names_its_session_and_agent() {
        let params = json!({
            "_meta": { "claudecode/sessionId": "cc-9", "claudecode/agentId": "agent-3" },
        });
        assert_eq!(
            identity(&params),
            named(Some("cc-9"), Some("agent-3"), None)
        );
    }

    /// Codex names the conversation's root thread, the agent's own thread
    /// when it differs, and the turn's git roots.
    #[test]
    fn codex_names_its_thread_and_git_root() {
        let params = json!({
            "_meta": {
                "sessionId": "root-1",
                "threadId": "thread-2",
                "x-codex-turn-metadata": {
                    "session_id": "root-1",
                    "workspaces": { "/home/me/repo": { "head": "abc" } },
                },
            },
        });
        assert_eq!(
            identity(&params),
            named(Some("root-1"), Some("thread-2"), Some("/home/me/repo"))
        );

        let main = json!({
            "_meta": {
                "sessionId": "root-1",
                "threadId": "root-1",
                "x-codex-turn-metadata": "{\"workspaces\":{\"/srv/x\":{}}}",
            },
        });
        assert_eq!(identity(&main), named(Some("root-1"), None, Some("/srv/x")));

        // Codex 0.155 sends no `sessionId`: the turn names the conversation.
        let older = json!({
            "_meta": {
                "threadId": "thread-2",
                "x-codex-turn-metadata": "{\"session_id\":\"root-1\",\"thread_id\":\"thread-2\"}",
            },
        });
        assert_eq!(
            identity(&older),
            named(Some("root-1"), Some("thread-2"), None)
        );
        let alone = json!({ "_meta": { "threadId": "root-1" } });
        assert_eq!(identity(&alone), named(Some("root-1"), None, None));
    }

    #[test]
    fn opencode_names_its_session() {
        let params = json!({ "_meta": { "ai.opencode/sessionID": "ses_1" } });
        assert_eq!(identity(&params), named(Some("ses_1"), None, None));
    }

    #[test]
    fn a_call_that_says_nothing_names_nothing() {
        let params = json!({ "name": "listen", "arguments": {} });
        assert_eq!(identity(&params), Named::default());
    }

    /// Blank values are none, long ones are cut, and a value that is not a
    /// string is ignored.
    #[test]
    fn what_is_kept_is_plain_text_of_a_bounded_length() {
        let long = "x".repeat(MAX_LEN + 50);
        let params = json!({
            "arguments": { "session": { "id": "  ", "agent": 7, "folder": long } },
        });
        let got = identity(&params);
        assert_eq!(got.id, None);
        assert_eq!(got.agent, None);
        assert_eq!(got.folder.map(|f| f.len()), Some(MAX_LEN));
    }
}
