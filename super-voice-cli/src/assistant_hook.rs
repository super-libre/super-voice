// SPDX-License-Identifier: GPL-3.0-only
//! `voice assistant-hook <assistant> <event>`: one of an AI assistant's
//! hooks, for an assistant whose hooks can only run a command. It reads the
//! hook's JSON on stdin, calls Super Voice's MCP server with the assistant's
//! key, and prints the hook's answer.
//!
//! Only the Antigravity CLI's hooks run it. It never stands in the
//! assistant's way: when Super Voice can't be reached it still prints an
//! answer that lets the assistant go on, and exits 0.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use super_voice_shared::daemon::assistant_mcp::call_tool;
use super_voice_shared::models::assistants::AssistantId;

/// The hook events the command answers.
pub(crate) const EVENTS: [&str; 4] = ["session-start", "pre-invocation", "pre-tool-use", "stop"];

/// The `assistant-hook` command.
pub(crate) fn command() -> clap::Command {
    use clap::{Arg, Command};
    Command::new("assistant-hook")
        .about("Run one of an AI assistant's hooks, which pass it the hook's JSON")
        .long_about(
            "Run one of an AI assistant's hooks: read the hook's JSON on stdin, tell Super \
             Voice, and print the hook's answer. The Antigravity CLI's hooks run this, once it \
             is connected on the Assistants page of the app. It always answers, and lets the \
             assistant go on when Super Voice can't be reached.",
        )
        // Only the Antigravity CLI's hooks can only run a command.
        .arg(
            Arg::new("assistant")
                .help("The assistant")
                .required(true)
                .value_parser(clap::builder::PossibleValuesParser::new([
                    AssistantId::Antigravity.as_wire_str(),
                ])),
        )
        .arg(
            Arg::new("event")
                .help("The hook's event")
                .required(true)
                .value_parser(clap::builder::PossibleValuesParser::new(EVENTS)),
        )
}

/// Super Voice's MCP server, as the assistant's config names it.
const SERVER: &str = "super-voice";

/// Answer `event`'s hook for `assistant`, with what Super Voice says.
pub(crate) async fn run(socket_path: PathBuf, assistant: AssistantId, event: &str) {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let hook: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let answer = match crate::assistant_access(socket_path, assistant).await {
        Some((key, address)) => antigravity(&address, &key, event, &hook).await,
        None => None,
    };
    println!("{}", answer.unwrap_or_else(|| going_on(event)));
}

/// The answer that lets the assistant go on as if there were no hook.
fn going_on(event: &str) -> Value {
    match event {
        "pre-tool-use" => json!({ "decision": "allow" }),
        "stop" => json!({ "decision": "stop" }),
        _ => json!({}),
    }
}

/// The conversation an Antigravity CLI hook is for, as Super Voice names a
/// session: its id, and the first folder it works in.
fn antigravity_session(hook: &Value) -> Value {
    let folder = hook["workspacePaths"][0]
        .as_str()
        .or_else(|| hook["cwd"].as_str());
    json!({ "id": hook["conversationId"], "folder": folder })
}

async fn antigravity(address: &str, key: &str, event: &str, hook: &Value) -> Option<Value> {
    let session = antigravity_session(hook);
    let call = |tool: &str, mut arguments: Value| {
        arguments["session"] = session.clone();
        let tool = tool.to_string();
        // A refused call is answered as if Super Voice could not be reached.
        async move {
            call_tool(address, key, &tool, arguments)
                .await
                .ok()
                .filter(|answer| !answer.is_error)
        }
    };
    match event {
        "session-start" => {
            call("session", json!({ "event": "seen" })).await;
            Some(json!({}))
        }
        "pre-invocation" => {
            let prompt = hook["lastUserInput"].as_str().unwrap_or_default();
            if prompt.trim().is_empty() {
                return Some(json!({}));
            }
            // The hook runs before each model call of a turn, numbered from
            // 0: past the first, it is the turn's prompt again.
            let again = hook["invocationNum"].as_u64().is_some_and(|n| n > 0);
            let answer = call(
                "turn",
                json!({ "event": "prompt", "text": prompt, "again": again }),
            )
            .await?;
            let turn: Value = serde_json::from_str(&answer.text).ok()?;
            Some(invocation(&turn))
        }
        "pre-tool-use" => Some(named_call(&hook["toolCall"], &session)),
        "stop" => {
            if hook["fullyIdle"] != false
                && let Some(reply) = hook["transcriptPath"].as_str().and_then(last_reply)
            {
                call("turn", json!({ "event": "reply", "text": reply })).await;
            }
            Some(going_on(event))
        }
        _ => None,
    }
}

/// The answer before a model call, from Super Voice's answer to its prompt:
/// a spoken prompt's note for the model; or, for a prompt Super Voice
/// answered itself, like `/read`, the turn ended before the model runs.
///
/// agy 1.5 shows the user a hook's text that the model never reads only as
/// an error, with a warning sign: a system message's `metadata` would, but
/// any `metadata` panics in its hook runner. So Super Voice's answer shows
/// only when it failed; when it worked, the user hears it.
fn invocation(turn: &Value) -> Value {
    if let Some(answered) = turn["answered"].as_str() {
        if turn["failed"] == false {
            return json!({
                // An empty, hidden step: agy's screen takes a turn that ends
                // with no new step for one that ended before it was sent, and
                // waits on it.
                "injectSteps": [{
                    "systemMessage": { "systemMessage": "", "renderInfo": { "hidden": true } },
                }],
                "terminationBehavior": "terminate",
            });
        }
        return json!({
            "injectSteps": [{ "errorMessage": { "userMessage": answered } }],
            "terminationBehavior": "terminate",
        });
    }
    match turn["note"].as_str() {
        Some(note) if turn["spoken"] == true => json!({
            "injectSteps": [{ "systemMessage": { "systemMessage": note } }],
        }),
        _ => json!({}),
    }
}

/// The answer to a tool call: `speak` and `listen` named as this
/// conversation's, so Super Voice holds them to its turn. agy calls an MCP
/// tool as `call_mcp_tool`, with each argument JSON in a string, or as
/// `mcp_<server>_<tool>`. What a hook overwrites is given as values, not as
/// JSON in strings: agy passes a string on as it is.
fn named_call(tool_call: &Value, session: &Value) -> Value {
    let args = &tool_call["args"];
    let decoded = |value: &Value| -> Value {
        value
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_else(|| value.clone())
    };
    let voice_tool = |tool: &str| tool == "speak" || tool == "listen";
    match tool_call["name"].as_str() {
        Some("call_mcp_tool")
            if decoded(&args["ServerName"]) == SERVER
                && decoded(&args["ToolName"]).as_str().is_some_and(voice_tool) =>
        {
            let mut arguments = decoded(&args["Arguments"]);
            if !arguments.is_object() {
                arguments = json!({});
            }
            arguments["session"] = session.clone();
            json!({
                "decision": "allow",
                "overwrite": { "Arguments": arguments },
            })
        }
        Some(name)
            if name
                .strip_prefix(&format!("mcp_{SERVER}_"))
                .is_some_and(voice_tool) =>
        {
            json!({ "decision": "allow", "overwrite": { "session": session } })
        }
        _ => json!({ "decision": "allow" }),
    }
}

/// The assistant's last reply in an Antigravity CLI transcript at `path`:
/// the last model response with text.
fn last_reply(path: &str) -> Option<String> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
        None => PathBuf::from(path),
    };
    reply_in(&std::fs::read_to_string(Path::new(&path)).ok()?)
}

fn reply_in(transcript: &str) -> Option<String> {
    let steps = transcript
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok());
    for step in steps {
        // The turn's own prompt: a turn that ended with no text has no
        // reply, and the one before it is the last turn's.
        if step["type"] == "USER_INPUT" {
            return None;
        }
        if step["type"] == "PLANNER_RESPONSE"
            && let Some(text) = step["content"].as_str()
            && !text.trim().is_empty()
        {
            return Some(text.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `speak` and `listen` are named as the conversation's, either way agy
    /// calls them; any other call goes on as it was.
    #[test]
    fn voice_calls_are_named_as_the_conversation() {
        let session = json!({ "id": "c1", "folder": "/work" });
        let lazy = json!({
            "name": "call_mcp_tool",
            "args": {
                "Arguments": "{\"text\":\"four\"}",
                "ServerName": "\"super-voice\"",
                "ToolName": "\"speak\"",
            },
        });
        let answer = named_call(&lazy, &session);
        assert_eq!(answer["decision"], "allow");
        assert_eq!(
            answer["overwrite"]["Arguments"],
            json!({ "text": "four", "session": session })
        );

        let eager = json!({ "name": "mcp_super-voice_listen", "args": {} });
        assert_eq!(
            named_call(&eager, &session)["overwrite"]["session"],
            session
        );

        for other in [
            json!({ "name": "run_command", "args": { "CommandLine": "ls" } }),
            json!({ "name": "call_mcp_tool", "args": { "ServerName": "\"docs\"", "ToolName": "\"speak\"" } }),
            json!({ "name": "mcp_super-voice_status", "args": {} }),
        ] {
            assert_eq!(
                named_call(&other, &session),
                json!({ "decision": "allow" }),
                "{other}"
            );
        }
    }

    /// The reply is the last model response with text.
    /// A spoken prompt's note goes to the model; a prompt Super Voice
    /// answered ends the turn, with its answer for the user alone.
    #[test]
    fn a_prompt_super_voice_answered_never_reaches_the_model() {
        let spoken = json!({ "spoken": true, "note": "Keep it short." });
        assert_eq!(
            invocation(&spoken),
            json!({ "injectSteps": [{ "systemMessage": { "systemMessage": "Keep it short." } }] })
        );
        assert_eq!(
            invocation(&json!({ "spoken": false, "note": null })),
            json!({})
        );
        // Heard, so nothing is shown.
        let read =
            json!({ "spoken": false, "answered": "Reading reply 1 aloud.", "failed": false });
        assert_eq!(
            invocation(&read),
            json!({
                "injectSteps": [{
                    "systemMessage": { "systemMessage": "", "renderInfo": { "hidden": true } },
                }],
                "terminationBehavior": "terminate",
            })
        );
        let unread = json!({ "spoken": false, "answered": "There is no reply to read yet.", "failed": true });
        assert_eq!(
            invocation(&unread),
            json!({
                "injectSteps": [{ "errorMessage": { "userMessage": "There is no reply to read yet." } }],
                "terminationBehavior": "terminate",
            })
        );
    }

    #[test]
    fn the_reply_is_the_last_model_text() {
        let transcript = [
            r#"{"type":"USER_INPUT","content":"<USER_REQUEST>hi</USER_REQUEST>"}"#,
            r#"{"type":"PLANNER_RESPONSE","content":"Looking."}"#,
            r#"{"type":"PLANNER_RESPONSE","tool_calls":[]}"#,
            r#"{"type":"GENERIC","content":"Speaking."}"#,
            r#"{"type":"PLANNER_RESPONSE","content":"done"}"#,
            "not json",
        ]
        .join("\n");
        assert_eq!(reply_in(&transcript).as_deref(), Some("done"));
        assert_eq!(reply_in(""), None);

        // A turn that ended with no text, interrupted or on a tool call, has
        // no reply: the last turn's is not this one's.
        let unanswered = [
            r#"{"type":"USER_INPUT","content":"<USER_REQUEST>hi</USER_REQUEST>"}"#,
            r#"{"type":"PLANNER_RESPONSE","content":"Hello."}"#,
            r#"{"type":"USER_INPUT","content":"<USER_REQUEST>go</USER_REQUEST>"}"#,
            r#"{"type":"PLANNER_RESPONSE","tool_calls":[]}"#,
        ]
        .join("\n");
        assert_eq!(reply_in(&unanswered), None);
    }

    #[test]
    fn a_hook_that_cannot_ask_lets_the_assistant_go_on() {
        assert_eq!(going_on("pre-tool-use"), json!({ "decision": "allow" }));
        assert_eq!(going_on("stop"), json!({ "decision": "stop" }));
        assert_eq!(going_on("session-start"), json!({}));
        assert_eq!(
            antigravity_session(&json!({ "conversationId": "c1", "workspacePaths": ["/w"] })),
            json!({ "id": "c1", "folder": "/w" })
        );
    }
}
