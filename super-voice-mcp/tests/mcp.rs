// SPDX-License-Identifier: GPL-3.0-only
//! End-to-end test for `super-voice-mcp`: the server on stdin and stdout,
//! driven the way an MCP client drives it.
//!
//! Hermetic like the CLI's `cmd_speak.rs`: the daemon comes up with no
//! backends, so the tools are refused before anything reaches an audio
//! device. `listen` is left out because it would open the microphone. The
//! server keeps its token in the test daemon's home, and the daemon
//! auto-approves it.
//!
//! ```bash
//! cargo test -p super-voice-mcp --test mcp -- --nocapture
//! ```

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};
use super_engine_test_daemon::TestDaemon;
use super_voice_shared::product::SUPER_VOICE;

const MCP_BIN: &str = env!("CARGO_BIN_EXE_super-voice-mcp");
/// Long enough for a debug daemon to answer, short enough that a server that
/// never answers fails the test rather than hanging it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// See the CLI's `cmd_speak.rs`.
fn locate_daemon_bin() -> PathBuf {
    let dir = PathBuf::from(MCP_BIN)
        .parent()
        .expect("mcp bin parent dir")
        .to_path_buf();
    let candidate = dir.join("super-voice-daemon");
    assert!(
        candidate.exists(),
        "expected the daemon binary at {} — run `cargo build -p super-voice-daemon` first",
        candidate.display()
    );
    candidate
}

/// See the CLI's `cmd_speak.rs`.
fn spawn_daemon() -> TestDaemon {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime to wait on the daemon")
        .block_on(TestDaemon::build(&SUPER_VOICE, locate_daemon_bin(), "mcp").start())
}

/// The server against `daemon`, with each line it writes on a channel.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
}

impl Server {
    fn start(daemon: &TestDaemon) -> Self {
        let mut child = Command::new(MCP_BIN)
            .env("XDG_STATE_HOME", &daemon.home().state)
            .env("SUPER_VOICE_AUTO_APPROVE", "1")
            .env("SUPER_VOICE_MUTE_CUES", "1")
            .args(["--socket", daemon.socket().to_str().expect("socket utf8")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn super-voice-mcp");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout pipe");
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("read a line");
                let message = serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("stdout carries only JSON-RPC; got `{line}`: {e}"));
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{message}").expect("write a line");
        stdin.flush().expect("flush");
    }

    fn request(&mut self, id: i64, method: &str, params: &Value) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let answer = self
            .lines
            .recv_timeout(ANSWER_TIMEOUT)
            .unwrap_or_else(|e| panic!("no answer to `{method}`: {e}"));
        assert_eq!(answer["id"], id, "answers `{method}`: {answer}");
        answer
    }
}

#[test]
fn mcp_serves_its_tools_and_reaches_the_daemon() {
    let daemon = spawn_daemon();
    let mut server = Server::start(&daemon);

    let init = server.request(
        1,
        "initialize",
        &json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0" },
        }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    server.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let list = server.request(2, "tools/list", &json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("a tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a tool name"))
        .collect();
    assert_eq!(names, ["speak", "listen", "synthesize"]);

    // No model, so the daemon refuses. The refusal comes back as a tool
    // error the model can read, not as a hang or a protocol error.
    let speak = server.request(
        3,
        "tools/call",
        &json!({ "name": "speak", "arguments": { "text": "Hello." } }),
    );
    assert_eq!(speak["result"]["isError"], true, "{speak}");
    assert!(
        speak["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "says why: {speak}"
    );

    let out = tempfile::tempdir().expect("a temp dir");
    let wav = out.path().join("hello.wav");
    let synthesize = server.request(
        4,
        "tools/call",
        &json!({
            "name": "synthesize",
            "arguments": { "text": "Hello.", "path": wav.to_str().expect("utf8 path") },
        }),
    );
    assert_eq!(synthesize["result"]["isError"], true, "{synthesize}");
    assert!(!wav.exists(), "a refused synthesis writes no file");

    // Closing stdin is how a client ends the session. A request sent just
    // before still gets its answer.
    server.send(&json!({ "jsonrpc": "2.0", "id": 5, "method": "ping" }));
    drop(server.stdin.take());
    let last = server
        .lines
        .recv_timeout(ANSWER_TIMEOUT)
        .expect("the last request is answered");
    assert_eq!(last, json!({ "jsonrpc": "2.0", "id": 5, "result": {} }));
    let status = server.child.wait().expect("wait super-voice-mcp");
    assert!(
        status.success(),
        "exits cleanly when stdin closes: {status}"
    );
}
