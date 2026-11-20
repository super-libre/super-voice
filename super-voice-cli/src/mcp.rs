// SPDX-License-Identifier: GPL-3.0-only
//! `voice mcp`: Super Voice as a Model Context Protocol server, so an AI
//! assistant such as Claude Code can talk to the user and hear them.
//!
//! The transport is MCP's stdio one: a JSON-RPC message per line on stdin, and
//! one per line on stdout. Stdout carries the protocol and nothing else;
//! logging goes to stderr.
//!
//! Only what a server of tools needs is here: `initialize`, `ping`,
//! `tools/list`, `tools/call` and `notifications/cancelled`. Each tool call
//! runs on a task of its own, so a `listen` waiting for the user does not hold
//! up a `ping`, and cancelling the call aborts the task. Aborting a `listen`
//! closes its connection, which the daemon takes as the user stopping the
//! recording.
//!
//! The server is a client of the daemon like any other, with a token of its
//! own. The consent dialog names the MCP client that started it, from
//! `initialize`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use super_voice_shared::daemon::http_client::{self, HttpResult, TranscribeOptions, transport};
use super_voice_shared::daemon::session::{self, AppId};
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::tts::daemon::http_client as tts_http;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::AbortHandle;

/// One token for every MCP client.
const APP_ID: AppId = session::app_id("super-voice-mcp");
/// `synthesize` hands audio in the user's voice out of the daemon, which
/// `speak` does not. `status` is how `listen` waits out its question.
const SCOPES: &[&str] = &["speak", "transcribe", "synthesize", "status"];
/// Who the consent dialog says is asking, when `initialize` named no client.
const UNNAMED_CLIENT: &str = "MCP client";

/// Newest first. A client asking for one of these gets it; any other gets the
/// newest, and decides whether it can work with that.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the client tells its model about this server.
const INSTRUCTIONS: &str = "Super Voice is the user's own speech service, running on their machine. \
`speak` says text aloud, `listen` hears what the user says, and `synthesize` saves speech to a WAV file. \
Use them when the user asks to talk by voice or to hear something; don't speak unasked. \
To ask the user something aloud, give the question to `listen` instead of calling `speak` first: \
`listen` waits for the question to end before it records. Keep spoken text short and plain.";

/// How often `listen` checks whether its question is still playing.
const POLL: Duration = Duration::from_millis(100);

const PARSE_ERROR: i64 = -32700;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Serve MCP on stdin and stdout until the client closes stdin.
///
/// # Errors
/// Returns an error if stdin can't be read.
pub async fn run(socket_path: PathBuf) -> Result<()> {
    let (out, mut outbox) = unbounded_channel::<Value>();
    // One writer, so two answers finishing at once never interleave a line.
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(message) = outbox.recv().await {
            let mut line = message.to_string();
            line.push('\n');
            if stdout.write_all(line.as_bytes()).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });
    let server = Arc::new(Server::new(socket_path));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        server.receive(&line, &out);
    }
    // The client is gone. What it is still waiting on is dropped, which ends
    // a `listen` as the user stopping it. What is already answered is
    // written, since a client may close stdin straight after its last request.
    for (_, call) in server.calls().drain() {
        call.abort();
    }
    drop(out);
    let _ = writer.await;
    Ok(())
}

struct Server {
    socket_path: PathBuf,
    /// Who the consent dialog says is asking.
    client_name: Mutex<String>,
    /// Tool calls in progress, by request id, for `notifications/cancelled`.
    calls: Mutex<HashMap<String, AbortHandle>>,
}

impl Server {
    fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            client_name: Mutex::new(UNNAMED_CLIENT.to_string()),
            calls: Mutex::new(HashMap::new()),
        }
    }

    fn calls(&self) -> MutexGuard<'_, HashMap<String, AbortHandle>> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Handle one line from the client.
    fn receive(self: &Arc<Self>, line: &str, out: &UnboundedSender<Value>) {
        if line.trim().is_empty() {
            return;
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(e) => {
                let _ = out.send(error(&Value::Null, PARSE_ERROR, &format!("Not JSON: {e}")));
                return;
            }
        };
        // No method: a response, and this server sends no requests.
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            self.notification(method, &params);
            return;
        };
        if method == "tools/call" {
            self.call(id, &params, out);
        } else {
            let _ = out.send(self.answer(&id, method, &params));
        }
    }

    /// The answer to every request but `tools/call`, which runs on a task of
    /// its own.
    fn answer(&self, id: &Value, method: &str, params: &Value) -> Value {
        match method {
            "initialize" => result(id, self.initialize(params)),
            "ping" => result(id, json!({})),
            "tools/list" => result(id, json!({ "tools": tools() })),
            _ => error(id, METHOD_NOT_FOUND, &format!("Unknown method: {method}")),
        }
    }

    fn notification(&self, method: &str, params: &Value) {
        if method != "notifications/cancelled" {
            return;
        }
        let Some(id) = params.get("requestId") else {
            return;
        };
        if let Some(call) = self.calls().remove(&id.to_string()) {
            call.abort();
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        let client = &params["clientInfo"];
        if let Some(name) = client["title"].as_str().or_else(|| client["name"].as_str()) {
            *self
                .client_name
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = format!("{name} (MCP)");
        }
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
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(self: &Arc<Self>, id: Value, params: &Value, out: &UnboundedSender<Value>) {
        let tool = match Tool::parse(params) {
            Ok(tool) => tool,
            Err(CallError::UnknownTool(name)) => {
                let _ = out.send(error(&id, INVALID_PARAMS, &format!("Unknown tool: {name}")));
                return;
            }
            Err(CallError::Arguments(message)) => {
                let _ = out.send(result(&id, tool_result(Err(message))));
                return;
            }
        };
        let key = id.to_string();
        // Held across the spawn, so a call that ends at once still finds its
        // own entry to remove.
        let mut calls = self.calls();
        let server = Arc::clone(self);
        let out = out.clone();
        let task = tokio::spawn(async move {
            let outcome = server.run(tool).await;
            server.calls().remove(&id.to_string());
            let _ = out.send(result(&id, tool_result(outcome)));
        });
        calls.insert(key, task.abort_handle());
    }

    async fn run(&self, tool: Tool) -> Result<String, String> {
        match tool {
            Tool::Speak { text } => self.speak(&text).await.map(|()| "Speaking.".to_string()),
            Tool::Listen { question } => self.listen(question.as_deref()).await,
            Tool::Synthesize { text, path } => self.synthesize(&text, &path).await,
        }
    }

    async fn speak(&self, text: &str) -> Result<(), String> {
        let resp = self
            .with_token(|token| {
                let socket = self.socket_path.clone();
                async move { tts_http::speak(socket, &token, text).await }
            })
            .await?;
        if resp.status == "success" {
            Ok(())
        } else {
            Err(resp
                .message
                .unwrap_or_else(|| "Could not speak.".to_string()))
        }
    }

    async fn listen(&self, question: Option<&str>) -> Result<String, String> {
        if let Some(question) = question {
            self.speak(question).await?;
            self.wait_for_silence().await?;
        }
        let resp = self
            .with_token(|token| {
                let socket = self.socket_path.clone();
                async move {
                    http_client::transcribe(
                        socket,
                        &token,
                        TranscribeOptions {
                            write_mode: false,
                            // Ends when the user stops talking, and their
                            // shortcut still stops it early.
                            stop_mode: Some(
                                RecordingStopMode::SilenceAndManual
                                    .as_wire_str()
                                    .to_string(),
                            ),
                            wait: true,
                            stream_realtime: false,
                        },
                    )
                    .await
                }
            })
            .await?;
        if resp.status != "success" {
            return Err(resp
                .message
                .unwrap_or_else(|| "Could not record.".to_string()));
        }
        Ok(resp
            .transcription
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "(no speech detected)".to_string()))
    }

    /// Wait until nothing is speaking. The daemon answers `speak` once the
    /// utterance is queued, not once it has played, and a recording started
    /// then would hear the question instead of the answer.
    async fn wait_for_silence(&self) -> Result<(), String> {
        loop {
            let status = self
                .with_token(|token| {
                    let socket = self.socket_path.clone();
                    async move { http_client::status(socket, &token).await }
                })
                .await?;
            if !status.tts.busy {
                return Ok(());
            }
            tokio::time::sleep(POLL).await;
        }
    }

    async fn synthesize(&self, text: &str, path: &Path) -> Result<String, String> {
        let body = json!({ "text": text });
        let wav = self
            .with_token(|token| {
                let socket = self.socket_path.clone();
                let body = &body;
                async move {
                    transport::post_json_for_bytes(socket, &token, "/tts/synthesize", body).await
                }
            })
            .await?;
        tokio::fs::write(path, &wav)
            .await
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(format!("Saved the speech to {}.", path.display()))
    }

    async fn with_token<F, Fut, T>(&self, op: F) -> Result<T, String>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = HttpResult<T>>,
    {
        let name = self
            .client_name
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        session::with_token(self.socket_path.clone(), APP_ID, &name, SCOPES, op)
            .await
            .map_err(|e| e.to_string())
    }
}

#[derive(Debug, PartialEq)]
enum Tool {
    Speak { text: String },
    Listen { question: Option<String> },
    Synthesize { text: String, path: PathBuf },
}

#[derive(Debug, PartialEq)]
enum CallError {
    /// A protocol error: the client asked for a tool `tools/list` never named.
    UnknownTool(String),
    /// A tool error, which the model sees and can correct.
    Arguments(String),
}

impl Tool {
    fn parse(params: &Value) -> Result<Self, CallError> {
        let args = &params["arguments"];
        match params["name"].as_str().unwrap_or_default() {
            "speak" => Ok(Self::Speak {
                text: required_text(args, "text")?,
            }),
            "listen" => Ok(Self::Listen {
                question: optional_text(args, "question")?,
            }),
            "synthesize" => {
                let text = required_text(args, "text")?;
                let path = PathBuf::from(required_text(args, "path")?);
                if !path.is_absolute() {
                    return Err(CallError::Arguments(format!(
                        "`path` must be absolute: {}",
                        path.display()
                    )));
                }
                Ok(Self::Synthesize { text, path })
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

fn tools() -> Value {
    json!([
        {
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
        },
        {
            "name": "listen",
            "title": "Listen",
            "description": "Record the user from the microphone until they stop talking, and return what they said. \
                Give a question to ask it aloud first; the recording starts when the question ends.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "A question to ask aloud before listening.",
                    },
                },
            },
        },
        {
            "name": "synthesize",
            "title": "Save speech",
            "description": "Speak text into a WAV file instead of out loud, in the user's voice.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "What to say." },
                    "path": {
                        "type": "string",
                        "description": "Absolute path of the WAV file to write. An existing file is replaced.",
                    },
                },
                "required": ["text", "path"],
            },
        },
    ])
}

fn tool_result(outcome: Result<String, String>) -> Value {
    let (text, is_error) = match outcome {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn result(id: &Value, result: Value) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "id": id });
    message["result"] = result;
    message
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_scopes_are_known() {
        use super_voice_shared::daemon::scopes::is_known_scope;
        for scope in SCOPES {
            assert!(
                is_known_scope(scope),
                "`voice mcp` requests scope `{scope}` that is not in scopes::known_scopes"
            );
        }
    }

    fn server() -> Arc<Server> {
        Arc::new(Server::new(PathBuf::from("/nonexistent/socket")))
    }

    /// Feed `lines` to a server and collect what it writes back.
    fn exchange(server: &Arc<Server>, lines: &[Value]) -> Vec<Value> {
        let (out, mut outbox) = unbounded_channel();
        for line in lines {
            server.receive(&line.to_string(), &out);
        }
        drop(out);
        let mut answers = Vec::new();
        while let Ok(answer) = outbox.try_recv() {
            answers.push(answer);
        }
        answers
    }

    #[test]
    fn initialize_agrees_on_a_version_the_client_asked_for() {
        let answers = exchange(
            &server(),
            &[json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "claude-code" } },
            })],
        );
        assert_eq!(answers.len(), 1);
        let result = &answers[0]["result"];
        assert_eq!(answers[0]["id"], 1);
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert_eq!(result["capabilities"], json!({ "tools": {} }));
        assert_eq!(result["serverInfo"]["name"], "super-voice");
    }

    #[test]
    fn initialize_offers_the_newest_version_for_one_it_does_not_know() {
        let answers = exchange(
            &server(),
            &[json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "1999-01-01" },
            })],
        );
        assert_eq!(
            answers[0]["result"]["protocolVersion"],
            PROTOCOL_VERSIONS[0]
        );
    }

    #[test]
    fn the_consent_dialog_names_the_client() {
        let server = server();
        assert_eq!(*server.client_name.lock().unwrap(), UNNAMED_CLIENT);
        exchange(
            &server,
            &[json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "clientInfo": { "name": "claude-code" } },
            })],
        );
        assert_eq!(*server.client_name.lock().unwrap(), "claude-code (MCP)");
        exchange(
            &server,
            &[json!({
                "jsonrpc": "2.0", "id": 2, "method": "initialize",
                "params": { "clientInfo": { "name": "x", "title": "Claude Code" } },
            })],
        );
        assert_eq!(*server.client_name.lock().unwrap(), "Claude Code (MCP)");
    }

    #[test]
    fn tools_list_names_each_tool_with_an_object_schema() {
        let answers = exchange(
            &server(),
            &[json!({ "jsonrpc": "2.0", "id": "a", "method": "tools/list" })],
        );
        assert_eq!(answers[0]["id"], "a");
        let tools = answers[0]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["speak", "listen", "synthesize"]);
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
        }
    }

    #[test]
    fn ping_answers_empty() {
        let answers = exchange(
            &server(),
            &[json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" })],
        );
        assert_eq!(
            answers,
            [json!({ "jsonrpc": "2.0", "id": 7, "result": {} })]
        );
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let answers = exchange(
            &server(),
            &[
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 3 } }),
                json!({ "jsonrpc": "2.0", "id": 9, "result": {} }),
            ],
        );
        assert!(answers.is_empty(), "{answers:?}");
    }

    #[test]
    fn an_unknown_method_is_an_error() {
        let answers = exchange(
            &server(),
            &[json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" })],
        );
        assert_eq!(answers[0]["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn a_line_that_is_not_json_is_a_parse_error() {
        let (out, mut outbox) = unbounded_channel();
        server().receive("{not json", &out);
        server().receive("   ", &out);
        drop(out);
        let answer = outbox.try_recv().unwrap();
        assert_eq!(answer["id"], Value::Null);
        assert_eq!(answer["error"]["code"], PARSE_ERROR);
        assert!(outbox.try_recv().is_err(), "a blank line is no message");
    }

    #[test]
    fn an_unknown_tool_is_a_protocol_error() {
        let answers = exchange(
            &server(),
            &[json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "shout", "arguments": {} },
            })],
        );
        assert_eq!(answers[0]["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn bad_arguments_are_a_tool_error_the_model_sees() {
        let answers = exchange(
            &server(),
            &[json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "speak", "arguments": { "text": "  " } },
            })],
        );
        assert_eq!(answers[0]["result"]["isError"], true);
        assert_eq!(
            answers[0]["result"]["content"][0]["text"],
            "`text` is required"
        );
    }

    #[test]
    fn tools_parse_their_arguments() {
        let call = |name: &str, arguments: Value| {
            Tool::parse(&json!({ "name": name, "arguments": arguments }))
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
        assert_eq!(
            call(
                "synthesize",
                json!({ "text": "Hi.", "path": "/tmp/hi.wav" })
            ),
            Ok(Tool::Synthesize {
                text: "Hi.".to_string(),
                path: PathBuf::from("/tmp/hi.wav"),
            })
        );
        assert!(matches!(
            call("synthesize", json!({ "text": "Hi.", "path": "hi.wav" })),
            Err(CallError::Arguments(_))
        ));
        // A client may leave `arguments` out of a call with none required.
        assert_eq!(
            Tool::parse(&json!({ "name": "listen" })),
            Ok(Tool::Listen { question: None })
        );
    }

    /// A call the daemon can't serve still answers, as a tool error, rather
    /// than leaving the client waiting.
    #[tokio::test]
    async fn a_call_with_no_daemon_answers_with_an_error() {
        let server = server();
        let (out, mut outbox) = unbounded_channel();
        server.receive(
            &json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "speak", "arguments": { "text": "Hello." } },
            })
            .to_string(),
            &out,
        );
        let answer = outbox.recv().await.unwrap();
        assert_eq!(answer["id"], 4);
        assert_eq!(answer["result"]["isError"], true);
        assert!(server.calls().is_empty(), "a finished call is forgotten");
    }

    #[tokio::test]
    async fn cancelling_a_call_aborts_it_and_sends_no_answer() {
        let server = server();
        let (out, mut outbox) = unbounded_channel::<Value>();
        let task = tokio::spawn(std::future::pending::<()>());
        server
            .calls()
            .insert(json!(5).to_string(), task.abort_handle());
        server.receive(
            &json!({
                "jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": { "requestId": 5, "reason": "user pressed Esc" },
            })
            .to_string(),
            &out,
        );
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(server.calls().is_empty());
        assert!(outbox.try_recv().is_err());
    }
}
