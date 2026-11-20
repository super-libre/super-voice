// SPDX-License-Identifier: GPL-3.0-only
//! MCP's protocol without its transport: what one JSON-RPC message asks for,
//! and the answer to all of them but a tool call, which [`super::tools`]
//! runs.
//!
//! Only what a server of tools needs is here: `initialize`, `ping`,
//! `tools/list`, `tools/call` and `notifications/cancelled`. The tools are
//! `speak` and `listen`, and a caller is offered only the ones its key or
//! token may use.

use serde_json::{Value, json};

/// Newest first. A client asking for one of these gets it; any other gets the
/// newest, and decides whether it can work with that.
pub(crate) const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// The body is not JSON.
pub(crate) const PARSE_ERROR: i64 = -32700;
/// The JSON is not a JSON-RPC message this server takes.
pub(crate) const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;

/// What the client tells its model about this server, around what it says of
/// when to speak.
const ABOUT: &str = "Super Voice is the user's own speech service, running on their machine. \
`speak` says text aloud, and `listen` hears what the user says. ";
const HOW_TO_ASK: &str = " To ask the user something aloud, give the question to `listen` instead of calling `speak` first: \
`listen` waits for the question to end before it records. Keep spoken text short and plain.";
/// When to speak, by default.
const WHEN_ASKED: &str =
    "Use them when the user asks to talk by voice or to hear something; don't speak unasked.";
/// When to speak, once the user lets the assistant ask its questions aloud.
const WHEN_ASKED_OR_ASKING: &str = "Use them when the user asks to talk by voice or to hear something. \
The user also lets you ask them questions aloud: when you need an answer from them, ask it with `listen`. \
Otherwise, don't speak unasked.";

/// What the client tells its model about this server. `asks_questions_aloud`
/// lets the model ask the user its questions aloud without being asked to
/// talk.
pub(crate) fn instructions(asks_questions_aloud: bool) -> String {
    let when = if asks_questions_aloud {
        WHEN_ASKED_OR_ASKING
    } else {
        WHEN_ASKED
    };
    format!("{ABOUT}{when}{HOW_TO_ASK}")
}

/// What the caller may do, from its key's or token's scopes, and what the
/// user chose for it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Access {
    /// It holds `speak`.
    pub(crate) may_speak: bool,
    /// It holds `transcribe`.
    pub(crate) may_listen: bool,
    /// The user lets it ask its questions aloud.
    pub(crate) asks_questions_aloud: bool,
}

/// What one message comes to.
#[derive(Debug, PartialEq)]
pub(crate) enum Handled {
    /// Answer with this message.
    Reply(Value),
    /// Not a message this server takes: refuse it with this error.
    Refuse(Value),
    /// A notification or a response: nothing to answer.
    Accepted,
    /// The client cancelled its request with this id: nothing to answer.
    Cancelled(Value),
    /// Run `tool`, and answer request `id` with what it comes to.
    Call { id: Value, tool: Tool },
}

/// A tool call to run.
#[derive(Debug, PartialEq)]
pub(crate) enum Tool {
    Speak { text: String },
    Listen { question: Option<String> },
}

/// Why a `tools/call` did not name a tool to run.
#[derive(Debug, PartialEq)]
enum CallError {
    /// A protocol error: the client asked for a tool `tools/list` did not
    /// offer it.
    UnknownTool(String),
    /// A tool error, which the model sees and can correct.
    Arguments(String),
}

/// The body read as JSON, or the parse error to answer with.
pub(crate) fn parse(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice(body)
        .map_err(|e| error(&Value::Null, PARSE_ERROR, &format!("Not JSON: {e}")))
}

/// What `message` comes to, for a caller with `access`.
pub(crate) fn handle(message: &Value, access: Access) -> Handled {
    let Some(object) = message.as_object() else {
        let what = if message.is_array() {
            "Batches are not taken: send one message per request"
        } else {
            "A message is a JSON object"
        };
        return Handled::Refuse(error(&Value::Null, INVALID_REQUEST, what));
    };
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // A response to a request of ours. This server sends none, so there
        // is nothing to do with one.
        if object.contains_key("result") || object.contains_key("error") {
            return Handled::Accepted;
        }
        return Handled::Refuse(error(&Value::Null, INVALID_REQUEST, "No method"));
    };
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = object.get("id").cloned() else {
        return notification(method, &params);
    };
    if !(id.is_string() || id.is_number()) {
        return Handled::Refuse(error(
            &Value::Null,
            INVALID_REQUEST,
            "A request's id is a string or a number",
        ));
    }
    match method {
        "initialize" => Handled::Reply(result(&id, initialize(&params, access))),
        "ping" => Handled::Reply(result(&id, json!({}))),
        "tools/list" => Handled::Reply(result(&id, json!({ "tools": tools(access) }))),
        "tools/call" => match Tool::parse(&params, access) {
            Ok(tool) => Handled::Call { id, tool },
            Err(CallError::UnknownTool(name)) => {
                Handled::Reply(error(&id, INVALID_PARAMS, &format!("Unknown tool: {name}")))
            }
            Err(CallError::Arguments(message)) => {
                Handled::Reply(result(&id, tool_result(Err(message))))
            }
        },
        _ => Handled::Reply(error(
            &id,
            METHOD_NOT_FOUND,
            &format!("Unknown method: {method}"),
        )),
    }
}

fn notification(method: &str, params: &Value) -> Handled {
    match (method, params.get("requestId")) {
        ("notifications/cancelled", Some(id)) => Handled::Cancelled(id.clone()),
        _ => Handled::Accepted,
    }
}

fn initialize(params: &Value, access: Access) -> Value {
    let version = params["protocolVersion"]
        .as_str()
        .filter(|asked| PROTOCOL_VERSIONS.contains(asked))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "super-voice",
            "title": "Super Voice",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": instructions(access.asks_questions_aloud),
    })
}

impl Tool {
    fn parse(params: &Value, access: Access) -> Result<Self, CallError> {
        let args = &params["arguments"];
        match params["name"].as_str().unwrap_or_default() {
            "speak" if access.may_speak => Ok(Self::Speak {
                text: required_text(args, "text")?,
            }),
            "listen" if access.may_listen => {
                let question = optional_text(args, "question")?;
                if question.is_some() && !access.may_speak {
                    return Err(CallError::Arguments(
                        "This assistant may not speak, so `listen` cannot ask a question \
                         aloud. Call `listen` without a question."
                            .to_string(),
                    ));
                }
                Ok(Self::Listen { question })
            }
            other => Err(CallError::UnknownTool(other.to_string())),
        }
    }
}

fn required_text(args: &Value, key: &str) -> Result<String, CallError> {
    optional_text(args, key)?.ok_or_else(|| CallError::Arguments(format!("`{key}` is required")))
}

fn optional_text(args: &Value, key: &str) -> Result<Option<String>, CallError> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(text) if text.trim().is_empty() => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(CallError::Arguments(format!("`{key}` must be a string"))),
    }
}

/// The tools a caller with `access` may use.
fn tools(access: Access) -> Vec<Value> {
    let mut tools = Vec::new();
    if access.may_speak {
        tools.push(json!({
            "name": "speak",
            "title": "Speak",
            "description": "Say text aloud on the user's speakers, in the voice they chose in Super Voice. \
                Returns once speech starts. Newer speech interrupts older.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "What to say. Markup is not read out, and a code block is said as \"code block\".",
                    },
                },
                "required": ["text"],
            },
        }));
    }
    if access.may_listen {
        let description = if access.may_speak {
            "Record the user from the microphone until they stop talking, and return what they said. \
             Give a question to ask it aloud first; the recording starts when the question ends."
        } else {
            "Record the user from the microphone until they stop talking, and return what they said."
        };
        let mut properties = serde_json::Map::new();
        if access.may_speak {
            properties.insert(
                "question".to_string(),
                json!({
                    "type": "string",
                    "description": "A question to ask aloud before listening.",
                }),
            );
        }
        tools.push(json!({
            "name": "listen",
            "title": "Listen",
            "description": description,
            "inputSchema": { "type": "object", "properties": properties },
        }));
    }
    tools
}

/// A tool's answer: its text, and whether it failed.
pub(crate) fn tool_result(outcome: Result<String, String>) -> Value {
    let (text, is_error) = match outcome {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// The answer to request `id`.
pub(crate) fn result(id: &Value, result: Value) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "id": id });
    message["result"] = result;
    message
}

/// The error answering request `id`, or `null` when there is none to name.
pub(crate) fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A caller that may speak and listen.
    fn both() -> Access {
        Access {
            may_speak: true,
            may_listen: true,
            asks_questions_aloud: false,
        }
    }

    fn reply(message: &Value, access: Access) -> Value {
        match handle(message, access) {
            Handled::Reply(reply) => reply,
            other => panic!("expected a reply to {message}, got {other:?}"),
        }
    }

    fn instructions_for(asks_questions_aloud: bool) -> String {
        let access = Access {
            asks_questions_aloud,
            ..both()
        };
        let answer = reply(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
            access,
        );
        answer["result"]["instructions"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn the_model_asks_aloud_only_once_the_user_lets_it() {
        let by_default = instructions_for(false);
        assert!(by_default.contains("don't speak unasked"), "{by_default}");
        assert!(
            !by_default.contains("ask them questions aloud"),
            "{by_default}"
        );

        let asking = instructions_for(true);
        assert!(asking.contains("ask them questions aloud"), "{asking}");
        // Both still say how to ask, and that the rest stays unasked.
        for text in [&by_default, &asking] {
            assert!(text.contains("give the question to `listen`"), "{text}");
            assert!(text.contains("speak unasked"), "{text}");
            assert!(!text.contains("synthesize"), "{text}");
        }
    }

    #[test]
    fn initialize_agrees_on_a_version_the_client_asked_for() {
        let answer = reply(
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "claude-code" } },
            }),
            both(),
        );
        let result = &answer["result"];
        assert_eq!(answer["id"], 1);
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert_eq!(result["capabilities"], json!({ "tools": {} }));
        assert_eq!(result["serverInfo"]["name"], "super-voice");
    }

    #[test]
    fn initialize_offers_the_newest_version_for_one_it_does_not_know() {
        for asked in ["1999-01-01", "2024-11-05"] {
            let answer = reply(
                &json!({
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": asked },
                }),
                both(),
            );
            assert_eq!(
                answer["result"]["protocolVersion"], PROTOCOL_VERSIONS[0],
                "{asked}"
            );
        }
    }

    #[test]
    fn tools_list_offers_only_what_the_caller_may_use() {
        let names = |access: Access| -> Vec<String> {
            let answer = reply(
                &json!({ "jsonrpc": "2.0", "id": "a", "method": "tools/list" }),
                access,
            );
            assert_eq!(answer["id"], "a");
            let tools = answer["result"]["tools"].as_array().unwrap().clone();
            for tool in &tools {
                assert_eq!(tool["inputSchema"]["type"], "object");
                assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
            }
            tools
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names(both()), ["speak", "listen"]);
        let speak_only = Access {
            may_listen: false,
            ..both()
        };
        assert_eq!(names(speak_only), ["speak"]);
        let listen_only = Access {
            may_speak: false,
            ..both()
        };
        assert_eq!(names(listen_only), ["listen"]);
        assert!(names(Access::default()).is_empty());
    }

    /// A caller that may not speak gives `listen` no question.
    #[test]
    fn listen_asks_no_question_for_a_caller_that_may_not_speak() {
        let listen_only = Access {
            may_speak: false,
            ..both()
        };
        let answer = reply(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            listen_only,
        );
        assert_eq!(
            answer["result"]["tools"][0]["inputSchema"]["properties"],
            json!({})
        );
        let refused = reply(
            &json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "listen", "arguments": { "question": "Which one?" } },
            }),
            listen_only,
        );
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert_eq!(
            handle(
                &json!({
                    "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                    "params": { "name": "listen", "arguments": {} },
                }),
                listen_only
            ),
            Handled::Call {
                id: json!(3),
                tool: Tool::Listen { question: None }
            }
        );
    }

    #[test]
    fn ping_answers_empty() {
        assert_eq!(
            reply(
                &json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" }),
                both()
            ),
            json!({ "jsonrpc": "2.0", "id": 7, "result": {} })
        );
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        for message in [
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": 9, "result": {} }),
            json!({ "jsonrpc": "2.0", "id": 9, "error": { "code": 1, "message": "no" } }),
        ] {
            assert_eq!(handle(&message, both()), Handled::Accepted, "{message}");
        }
    }

    #[test]
    fn a_cancel_names_the_request_it_cancels() {
        assert_eq!(
            handle(
                &json!({
                    "jsonrpc": "2.0", "method": "notifications/cancelled",
                    "params": { "requestId": 5, "reason": "user pressed Esc" },
                }),
                both()
            ),
            Handled::Cancelled(json!(5))
        );
    }

    #[test]
    fn an_unknown_method_is_an_error() {
        let answer = reply(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
            both(),
        );
        assert_eq!(answer["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn a_body_that_is_not_json_is_a_parse_error() {
        let answer = parse(b"{not json").unwrap_err();
        assert_eq!(answer["id"], Value::Null);
        assert_eq!(answer["error"]["code"], PARSE_ERROR);
    }

    #[test]
    fn json_that_is_not_one_message_is_refused() {
        for message in [
            json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }]),
            json!("ping"),
            json!({ "jsonrpc": "2.0", "id": 1 }),
            json!({ "jsonrpc": "2.0", "id": null, "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": {}, "method": "ping" }),
        ] {
            match handle(&message, both()) {
                Handled::Refuse(answer) => {
                    assert_eq!(answer["error"]["code"], INVALID_REQUEST, "{message}");
                }
                other => panic!("{message} came to {other:?}"),
            }
        }
    }

    #[test]
    fn an_unknown_tool_is_a_protocol_error() {
        for name in ["shout", "synthesize"] {
            let answer = reply(
                &json!({
                    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": { "name": name, "arguments": {} },
                }),
                both(),
            );
            assert_eq!(answer["error"]["code"], INVALID_PARAMS, "{name}");
        }
        // One the caller may not use is not offered, so it is unknown too.
        let speak_only = Access {
            may_listen: false,
            ..both()
        };
        let answer = reply(
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "listen" },
            }),
            speak_only,
        );
        assert_eq!(answer["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn bad_arguments_are_a_tool_error_the_model_sees() {
        let answer = reply(
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "speak", "arguments": { "text": "  " } },
            }),
            both(),
        );
        assert_eq!(answer["result"]["isError"], true);
        assert_eq!(answer["result"]["content"][0]["text"], "`text` is required");
    }

    #[test]
    fn tools_parse_their_arguments() {
        let call = |name: &str, arguments: Value| {
            Tool::parse(&json!({ "name": name, "arguments": arguments }), both())
        };
        assert_eq!(
            call("speak", json!({ "text": "Hello." })),
            Ok(Tool::Speak {
                text: "Hello.".to_string()
            })
        );
        assert_eq!(
            call("listen", json!({})),
            Ok(Tool::Listen { question: None })
        );
        assert_eq!(
            call("listen", json!({ "question": "Which one?" })),
            Ok(Tool::Listen {
                question: Some("Which one?".to_string())
            })
        );
        assert_eq!(
            call("listen", json!({ "question": 3 })),
            Err(CallError::Arguments(
                "`question` must be a string".to_string()
            ))
        );
        // A client may leave `arguments` out of a call with none required.
        assert_eq!(
            Tool::parse(&json!({ "name": "listen" }), both()),
            Ok(Tool::Listen { question: None })
        );
    }

    #[test]
    fn a_tool_result_says_whether_it_failed() {
        assert_eq!(
            tool_result(Ok("Speaking.".to_string())),
            json!({ "content": [{ "type": "text", "text": "Speaking." }], "isError": false })
        );
        assert_eq!(tool_result(Err("No model.".to_string()))["isError"], true);
    }
}
