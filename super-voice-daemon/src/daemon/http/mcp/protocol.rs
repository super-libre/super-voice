// SPDX-License-Identifier: GPL-3.0-only
//! MCP's protocol without its transport: what one JSON-RPC message asks for,
//! and the answer to all of them but a tool call, which [`super::tools`]
//! runs.
//!
//! Only what a server of tools needs is here: `initialize`, `ping`,
//! `tools/list`, `tools/call` and `notifications/cancelled`. The tools are
//! `speak` and `listen`, and a caller is offered only the ones its key or
//! token may use. A connected assistant's plugin also gets `stop`, `status`,
//! `announce`, `session` and `turn`, kept from the model: see [`Plugin`].

use serde_json::{Value, json};
use super_voice_shared::models::contexts::ExtraContext;

/// Newest first. A client asking for one of these gets it; any other gets the
/// newest, and decides whether it can work with that. Not 2025-03-26, which
/// has servers take batches of messages: this one takes one at a time.
pub(crate) const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18"];

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
`listen` waits for the question to end before it records. Give `listen` a `context` with the names and terms \
of your conversation the user may say. Keep spoken text short and plain.";
/// When to speak: only after being spoken to.
const WHEN_SPOKEN_TO: &str = "Use them only while the user is talking to you by voice. \
Answer a prompt they typed in writing, even one that asks you to speak.";

/// What the client tells its model about this server.
pub(crate) fn instructions() -> String {
    format!("{ABOUT}{WHEN_SPOKEN_TO}{HOW_TO_ASK}")
}

/// What the caller may do, from its key's or token's scopes, and which
/// assistant it is.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Access {
    /// It holds `speak`.
    pub(crate) may_speak: bool,
    /// It holds `transcribe`.
    pub(crate) may_listen: bool,
    /// Its assistant's plugin, which calls tools of its own.
    pub(crate) plugin: Plugin,
}

/// An assistant's plugin that works through its own connection, and how the
/// tools it calls itself are kept from the assistant's model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Plugin {
    /// No plugin: the model's tools alone.
    #[default]
    None,
    /// Claude Code's, `OpenCode`'s and agy's, which hide them from the model
    /// themselves. Claude Code keeps a tool marked for the app alone from
    /// the plugin's own calls too, so they go unmarked.
    HidesItsTools,
    /// Codex's: its client keeps a tool marked for the app alone from its
    /// model, so they are marked.
    ReadsTheMark,
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
    Speak {
        text: String,
        /// The user asked for it themselves, through their plugin, as with
        /// a reply's button that reads it again: a typed turn does not
        /// refuse it.
        asked: bool,
    },
    Listen {
        question: Option<String>,
        /// What the conversation tells about what the user will say.
        context: Option<ExtraContext>,
        /// The user asked for it themselves, through their plugin's Talk: a
        /// typed turn does not refuse it.
        asked: bool,
    },
    /// The plugin's: stop the session's own speech and recording.
    Stop,
    /// The plugin's: whether the session's own speech plays, and whether it
    /// records.
    Status,
    /// The plugin's: say a notice, unless the user turned announcing off.
    Announce { text: String },
    /// The plugin's: the session started, ended, or is still there.
    Session(SessionEvent),
    /// The plugin's: the user sent a prompt, or the assistant answered it.
    Turn(TurnEvent),
}

/// What a plugin says of its session.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum SessionEvent {
    /// It started, and checks in while it is open.
    Start,
    /// It ended.
    End,
    /// It is there, from a plugin that cannot check in, like a hook's.
    Seen,
}

/// A turn of a session: the prompt that starts it, and the reply that ends
/// it.
#[derive(Debug, PartialEq)]
pub(crate) enum TurnEvent {
    /// The user sent `text`. `spoken` when the plugin knows whether it was
    /// said aloud; when it does not, the daemon tells: from `heard`, the
    /// words the plugin's Talk put in the prompt box, which the user may have
    /// touched up, or from its dictation. `again` when a hook that runs
    /// before each model call says the turn's prompt again. `/read` asks to
    /// hear a reply again instead, and is answered by the daemon.
    Prompt {
        text: String,
        spoken: Option<bool>,
        heard: Option<String>,
        again: bool,
    },
    /// The assistant answered with `text`, kept to read again, and read
    /// aloud when the turn was spoken.
    Reply { text: String },
}

/// The form a hook takes a tool's answer in, when a hook calls the tool
/// itself and reads its text as its own output.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Hook {
    /// Codex's `mcp_tool` hooks: nothing, or the hook's JSON.
    Codex,
}

/// The hook a call comes from, by its `hook` argument.
pub(crate) fn hook_of(params: &Value) -> Option<Hook> {
    match params["arguments"]["hook"].as_str()? {
        "codex" => Some(Hook::Codex),
        _ => None,
    }
}

impl Tool {
    /// Its name, as the client calls it.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Speak { .. } => "speak",
            Self::Listen { .. } => "listen",
            Self::Stop => "stop",
            Self::Status => "status",
            Self::Announce { .. } => "announce",
            Self::Session(_) => "session",
            Self::Turn(_) => "turn",
        }
    }
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
        "initialize" => Handled::Reply(result(&id, initialize(&params))),
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

fn initialize(params: &Value) -> Value {
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
        "instructions": instructions(),
    })
}

impl Tool {
    fn parse(params: &Value, access: Access) -> Result<Self, CallError> {
        let args = &params["arguments"];
        match params["name"].as_str().unwrap_or_default() {
            "speak" if access.may_speak => Ok(Self::Speak {
                text: required_text(args, "text")?,
                asked: args["asked"].as_bool().unwrap_or(false),
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
                Ok(Self::Listen {
                    question,
                    context: listen_context(&args["context"])?,
                    asked: args["asked"].as_bool().unwrap_or(false),
                })
            }
            "stop" if access.plugin != Plugin::None => Ok(Self::Stop),
            "status" if access.plugin != Plugin::None => Ok(Self::Status),
            // `message` is what a hook's notification carries.
            "announce" if access.plugin != Plugin::None && access.may_speak => Ok(Self::Announce {
                text: match optional_text(args, "text")? {
                    Some(text) => text,
                    None => required_text(args, "message")
                        .map_err(|_| CallError::Arguments("`text` is required".to_string()))?,
                },
            }),
            "session" if access.plugin != Plugin::None => {
                match optional_text(args, "event")?.as_deref() {
                    Some("start") => Ok(Self::Session(SessionEvent::Start)),
                    Some("end") => Ok(Self::Session(SessionEvent::End)),
                    Some("seen") => Ok(Self::Session(SessionEvent::Seen)),
                    _ => Err(CallError::Arguments(
                        "`event` is \"start\", \"end\" or \"seen\"".to_string(),
                    )),
                }
            }
            "turn" if access.plugin != Plugin::None => {
                // A hook may hand over nothing for a reply it lacks.
                let text = optional_text(args, "text")?.unwrap_or_default();
                match optional_text(args, "event")?.as_deref() {
                    Some("prompt") => Ok(Self::Turn(TurnEvent::Prompt {
                        text,
                        spoken: args["spoken"].as_bool(),
                        heard: optional_text(args, "heard")?,
                        again: args["again"].as_bool() == Some(true),
                    })),
                    Some("reply") => Ok(Self::Turn(TurnEvent::Reply { text })),
                    _ => Err(CallError::Arguments(
                        "`event` is \"prompt\" or \"reply\"".to_string(),
                    )),
                }
            }
            other => Err(CallError::UnknownTool(other.to_string())),
        }
    }
}

/// `listen`'s `context`. Lenient where `POST /stt/transcribe` is strict, so
/// a model's call never fails on its context: a term that is not text is
/// skipped, and what is past a context's limits is cut. Only a `context`
/// that is not an object is refused, as a mistake the model can fix.
fn listen_context(value: &Value) -> Result<Option<ExtraContext>, CallError> {
    let fields = match value {
        Value::Null => return Ok(None),
        Value::Object(fields) => fields,
        _ => {
            return Err(CallError::Arguments(
                "`context` is {\"prompt\": text, \"vocabulary\": [text]}".to_string(),
            ));
        }
    };
    let raw = ExtraContext {
        prompt: fields
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        vocabulary: fields
            .get("vocabulary")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    };
    // Added to nothing, it is cut to the limits.
    let fitted = raw.added_to(None);
    let context = ExtraContext {
        prompt: fitted.prompt,
        vocabulary: fitted.vocabulary,
    };
    Ok((!context.is_empty()).then_some(context))
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
                Returns once all of it is made, while its end may still be playing. Newer speech \
                interrupts older.",
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
            "annotations": local_audio(),
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
        properties.insert(
            "context".to_string(),
            json!({
                "type": "object",
                "description": "What the user is likely to say, from your conversation with them. \
                    Helps Super Voice hear names and terms it would otherwise get wrong.",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "One line on what the user is about to talk about.",
                    },
                    "vocabulary": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Names, files, commands and terms from the conversation \
                            the user may say, spelled as they should be written. Up to 50.",
                    },
                },
            }),
        );
        tools.push(json!({
            "name": "listen",
            "title": "Listen",
            "description": description,
            "inputSchema": { "type": "object", "properties": properties },
            "annotations": local_audio(),
        }));
    }
    if access.plugin != Plugin::None {
        tools.extend(plugin_tools(
            access.may_speak,
            access.plugin == Plugin::ReadsTheMark,
        ));
    }
    tools
}

/// What `speak` and `listen` do, as MCP's tool annotations say it: they play
/// or record audio on this machine, and change nothing else. A client may
/// run such a tool without asking the user first, as Codex does; otherwise
/// each spoken answer would wait on an approval.
fn local_audio() -> Value {
    json!({
        "readOnlyHint": false,
        "destructiveHint": false,
        "idempotentHint": false,
        "openWorldHint": false,
    })
}

/// The tools an assistant's plugin calls itself. With `marked`, each is
/// marked for the app alone, which Codex reads to keep it from its model.
/// Claude Code reads the mark too, and then refuses its plugin's own calls,
/// so its plugin hides them itself instead.
fn plugin_tools(may_speak: bool, marked: bool) -> Vec<Value> {
    let empty = json!({ "type": "object", "properties": {} });
    let mut tools = vec![
        json!({
            "name": "stop",
            "title": "Stop",
            "description": "Stop this session's own speech, and end its recording: what it heard so far is \
                still transcribed. Another session's speech and recording are left alone.",
            "inputSchema": empty,
        }),
        json!({
            "name": "status",
            "title": "Status",
            "description": "Whether this session's own speech is playing, and whether it is recording, as \
                {\"speaking\": bool, \"listening\": bool}.",
            "inputSchema": empty,
        }),
        json!({
            "name": "session",
            "title": "Session",
            "description": "Say that this session started or ended, so Super Voice lists it while it is open.",
            "inputSchema": {
                "type": "object",
                "properties": { "event": { "type": "string", "enum": ["start", "end", "seen"] } },
                "required": ["event"],
            },
        }),
        json!({
            "name": "turn",
            "title": "Turn",
            "description": "Say that the user sent a prompt, and whether it was spoken, or that the \
                assistant answered it. A spoken turn's reply is read aloud. A prompt of `/read` or \
                `$read`, with a number from 0 to 9, reads one of the last replies aloud again \
                instead, and is answered here rather than by the assistant.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event": { "type": "string", "enum": ["prompt", "reply"] },
                    "text": { "type": "string" },
                    "spoken": { "type": "boolean" },
                    "heard": { "type": "string" },
                    "again": { "type": "boolean" },
                },
                "required": ["event"],
            },
        }),
    ];
    if may_speak {
        tools.push(json!({
            "name": "announce",
            "title": "Announce",
            "description": "Say a short notice aloud, like that the assistant needs the user's permission, \
                unless the user turned announcing off for this assistant.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string", "description": "The notice." } },
                "required": ["text"],
            },
        }));
    }
    if marked {
        for tool in &mut tools {
            tool["_meta"] = json!({ "ui": { "visibility": ["app"] } });
        }
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
            plugin: Plugin::None,
        }
    }

    fn reply(message: &Value, access: Access) -> Value {
        match handle(message, access) {
            Handled::Reply(reply) => reply,
            other => panic!("expected a reply to {message}, got {other:?}"),
        }
    }

    /// The model is told to speak only after being spoken to, and how to ask
    /// a question aloud then.
    #[test]
    fn the_model_speaks_only_after_being_spoken_to() {
        let answer = reply(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
            both(),
        );
        let text = answer["result"]["instructions"].as_str().unwrap();
        assert!(
            text.contains("only while the user is talking to you by voice"),
            "{text}"
        );
        assert!(text.contains("even one that asks you to speak"), "{text}");
        assert!(text.contains("give the question to `listen`"), "{text}");
        assert!(!text.contains("synthesize"), "{text}");
    }

    /// The plugin's tools are offered only to an assistant whose plugin
    /// works through its own connection, each marked for the app alone.
    #[test]
    fn the_plugins_tools_are_offered_only_with_a_plugin() {
        let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let names = |access: Access| -> Vec<String> {
            reply(&list, access)["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names(both()), ["speak", "listen"]);
        let plugin = Access {
            plugin: Plugin::HidesItsTools,
            ..both()
        };
        // The plugin's own are the ones an assistant's config keeps from
        // its model.
        let mut plugin_names = names(plugin);
        assert_eq!(plugin_names[..2], ["speak", "listen"]);
        let mut kept = crate::assistants::PLUGIN_TOOLS.to_vec();
        plugin_names[2..].sort_unstable();
        kept.sort_unstable();
        assert_eq!(plugin_names[2..], kept);
        // Claude Code's are not marked: it would refuse its plugin's calls.
        let tools = reply(&list, plugin)["result"]["tools"].clone();
        assert_eq!(tools[0].get("_meta"), None, "the model's own tools");
        assert_eq!(tools[2].get("_meta"), None, "{}", tools[2]);
        // Codex's are, so it keeps them from its model.
        let codex = Access {
            plugin: Plugin::ReadsTheMark,
            ..plugin
        };
        let tools = reply(&list, codex)["result"]["tools"].clone();
        assert_eq!(tools[0].get("_meta"), None, "the model's own tools");
        assert_eq!(tools[2]["_meta"]["ui"]["visibility"], json!(["app"]));
        let quiet = Access {
            may_speak: false,
            ..plugin
        };
        assert!(!names(quiet).contains(&"announce".to_string()));
    }

    /// The `session` argument names the caller and is not the tool's own;
    /// `announce` takes a hook's `message` as its text.
    #[test]
    fn the_plugins_calls_parse() {
        let plugin = Access {
            plugin: Plugin::HidesItsTools,
            ..both()
        };
        let call = |name: &str, arguments: Value| {
            handle(
                &json!({
                    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": { "name": name, "arguments": arguments },
                }),
                plugin,
            )
        };
        let session = json!({ "id": "s1" });
        assert_eq!(
            call("speak", json!({ "text": "Hi.", "session": session })),
            Handled::Call {
                id: json!(1),
                tool: Tool::Speak {
                    text: "Hi.".to_string(),
                    asked: false,
                }
            }
        );
        assert_eq!(
            call(
                "announce",
                json!({ "message": "Claude needs your permission" })
            ),
            Handled::Call {
                id: json!(1),
                tool: Tool::Announce {
                    text: "Claude needs your permission".to_string()
                }
            }
        );
        assert_eq!(
            call("session", json!({ "event": "end" })),
            Handled::Call {
                id: json!(1),
                tool: Tool::Session(SessionEvent::End)
            }
        );
        // A hook that runs before each model call says the turn's prompt
        // again; a prompt is a new turn otherwise.
        for (again, sent) in [
            (true, json!(true)),
            (false, json!(false)),
            (false, Value::Null),
        ] {
            assert_eq!(
                call(
                    "turn",
                    json!({ "event": "prompt", "text": "Go.", "again": sent })
                ),
                Handled::Call {
                    id: json!(1),
                    tool: Tool::Turn(TurnEvent::Prompt {
                        text: "Go.".to_string(),
                        spoken: None,
                        heard: None,
                        again,
                    })
                }
            );
        }
        let Handled::Reply(refused) = call("session", json!({ "event": "later" })) else {
            panic!("a bad event is the model's error");
        };
        assert_eq!(refused["result"]["isError"], true);
        let Handled::Reply(unknown) = handle(
            &json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "stop", "arguments": {} },
            }),
            both(),
        ) else {
            panic!("a reply");
        };
        assert_eq!(
            unknown["error"]["code"], INVALID_PARAMS,
            "not offered without a plugin"
        );
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
        for asked in ["1999-01-01", "2024-11-05", "2025-03-26"] {
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

    /// `speak` and `listen` say they change nothing and reach nothing
    /// outside this machine, so a client such as Codex runs them without
    /// asking the user first.
    #[test]
    fn speaking_and_listening_need_no_approval() {
        let answer = reply(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            both(),
        );
        for tool in answer["result"]["tools"].as_array().unwrap() {
            let annotations = &tool["annotations"];
            assert_eq!(annotations["destructiveHint"], false, "{tool}");
            assert_eq!(annotations["openWorldHint"], false, "{tool}");
        }
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
        // It takes the conversation's context still, and no question.
        let properties = &answer["result"]["tools"][0]["inputSchema"]["properties"];
        assert!(properties.get("question").is_none(), "{properties}");
        assert_eq!(properties["context"]["type"], "object", "{properties}");
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
                tool: Tool::Listen {
                    question: None,
                    context: None,
                    asked: false
                }
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
                text: "Hello.".to_string(),
                asked: false,
            })
        );
        assert_eq!(
            call("listen", json!({})),
            Ok(Tool::Listen {
                question: None,
                context: None,
                asked: false
            })
        );
        assert_eq!(
            call("listen", json!({ "question": "Which one?" })),
            Ok(Tool::Listen {
                question: Some("Which one?".to_string()),
                context: None,
                asked: false
            })
        );
        // What the user asked for through their plugin: Talk, or reading a
        // reply again.
        assert_eq!(
            call("speak", json!({ "text": "Hello.", "asked": true })),
            Ok(Tool::Speak {
                text: "Hello.".to_string(),
                asked: true,
            })
        );
        assert_eq!(
            call("listen", json!({ "asked": true })),
            Ok(Tool::Listen {
                question: None,
                context: None,
                asked: true
            })
        );
        assert_eq!(
            call("listen", json!({ "question": 3 })),
            Err(CallError::Arguments(
                "`question` must be a string".to_string()
            ))
        );
        // A context is taken as it can be: a term that is not text is
        // skipped, and one that adds nothing is none.
        let heard = |context: Value| match call("listen", json!({ "context": context })) {
            Ok(Tool::Listen { context, .. }) => Ok(context),
            other => Err(other),
        };
        assert_eq!(
            heard(json!({ "prompt": " The release. ", "vocabulary": ["agy", 3, " "] })),
            Ok(Some(ExtraContext {
                prompt: "The release.".to_string(),
                vocabulary: vec!["agy".to_string()],
            }))
        );
        assert_eq!(heard(json!({ "prompt": 3 })), Ok(None));
        // Past a context's limits, it is cut rather than refused.
        let long = "a".repeat(super_voice_shared::models::contexts::MAX_PROMPT_CHARS + 10);
        let cut = heard(json!({ "prompt": long })).expect("a context");
        assert_eq!(
            cut.map(|c| c.prompt.chars().count()),
            Some(super_voice_shared::models::contexts::MAX_PROMPT_CHARS)
        );
        assert!(matches!(
            call("listen", json!({ "context": "agy" })),
            Err(CallError::Arguments(_))
        ));
        // A client may leave `arguments` out of a call with none required.
        assert_eq!(
            Tool::parse(&json!({ "name": "listen" }), both()),
            Ok(Tool::Listen {
                question: None,
                context: None,
                asked: false
            })
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
