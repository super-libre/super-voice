// SPDX-License-Identifier: GPL-3.0-only
//! `/v1/mcp` end to end: Super Voice's MCP server on a real daemon, which has
//! no model installed.
//!
//! Over the Unix socket a session token reaches it, as a test or the CLI
//! does. Over the TCP listener an assistant reaches it with its key, and with
//! nothing else: not with an `Origin`, and not with a session token. The
//! daemon runs in a home of its own, with `HOME` the test's, so connecting an
//! assistant writes only there.

mod common;

use common::{Method, StatusCode, TestDaemon};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1::handshake;
use serde_json::{Value, json};
use std::path::PathBuf;
use super_voice_shared::daemon::http_client;

/// What `POST /v1/mcp` answered: the status, the content type, and the body.
struct Answer {
    status: StatusCode,
    content_type: String,
    body: String,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }

    /// The JSON-RPC message in the stream's one `message` event.
    fn message(&self) -> Value {
        assert_eq!(self.content_type, "text/event-stream", "{}", self.body);
        assert!(self.body.starts_with(": stream-open"), "{}", self.body);
        let data = self
            .body
            .split("\n\n")
            .find_map(|frame| {
                frame
                    .strip_prefix("event: message\ndata: ")
                    .map(str::to_string)
            })
            .unwrap_or_else(|| panic!("no message event: {}", self.body));
        serde_json::from_str(&data).expect("the event's data is JSON")
    }
}

/// Where a request goes.
enum Via<'a> {
    Socket(&'a PathBuf),
    Tcp(u16),
}

/// `method /v1/mcp`, with `body`, the bearer `token`, and `headers`.
async fn send(
    via: &Via<'_>,
    method: Method,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: &str,
) -> Answer {
    let host = match via {
        Via::Socket(_) => "voice.local".to_string(),
        Via::Tcp(port) => format!("127.0.0.1:{port}"),
    };
    let mut request = hyper::Request::builder()
        .method(method)
        .uri("/v1/mcp")
        .header("host", host)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(Full::new(Bytes::from(body.to_string())))
        .expect("build request");
    let response = match via {
        Via::Socket(socket) => {
            let stream = tokio::net::UnixStream::connect(socket)
                .await
                .expect("connect");
            exchange(stream, request).await
        }
        Via::Tcp(port) => {
            let stream = tokio::net::TcpStream::connect(("127.0.0.1", *port))
                .await
                .expect("connect over TCP");
            exchange(stream, request).await
        }
    };
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read the body")
        .to_bytes();
    Answer {
        status,
        content_type,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn exchange<S>(
    stream: S,
    request: hyper::Request<Full<Bytes>>,
) -> hyper::Response<hyper::body::Incoming>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let io = hyper_util::rt::TokioIo::new(stream);
    let (mut sender, connection) = handshake::<_, Full<Bytes>>(io).await.expect("handshake");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender.send_request(request).await.expect("send request")
}

async fn post(via: &Via<'_>, token: &str, message: &Value) -> Answer {
    send(via, Method::POST, Some(token), &[], &message.to_string()).await
}

/// A daemon in a home of its own, which finds Codex there, with its TCP
/// listener on `port` when given.
async fn start(label: &str, port: Option<u16>) -> TestDaemon {
    let builder = common::daemon(label);
    let home = builder.home().root().join("home");
    std::fs::create_dir_all(home.join(".codex")).expect("mkdir .codex");
    if let Some(port) = port {
        let config = builder.home().config.join("super-voice/daemon.ron");
        std::fs::create_dir_all(config.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &config,
            format!(
                r#"(
    audio: (theme: "classic", volume: 100),
    transcription: (preferred_model: ""),
    http: (tcp: (port: {port})),
)"#
            ),
        )
        .expect("write config");
    }
    builder
        .env("HOME", &home)
        .without("CODEX_HOME")
        .env("SUPER_VOICE_TREAT_AS_OFFICIAL", "1")
        // No desktop notification reaches whoever runs the suite.
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
        .start()
        .await
}

async fn token(daemon: &TestDaemon, scopes: &[&str]) -> String {
    http_client::auth_request(daemon.socket().to_path_buf(), "mcp test", scopes)
        .await
        .expect("auth_request")
        .session_token
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" },
        },
    })
}

fn tool_names(answer: &Answer) -> Vec<String> {
    answer.json()["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_string())
        .collect()
}

#[tokio::test]
async fn a_session_starts_and_lists_the_tools_it_may_use() {
    let daemon = start("mcp-session", None).await;
    let socket = daemon.socket().to_path_buf();
    let via = Via::Socket(&socket);
    let both = token(&daemon, &["speak", "transcribe"]).await;

    let answer = post(&via, &both, &initialize()).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.content_type, "application/json");
    let result = &answer.json()["result"];
    assert_eq!(result["protocolVersion"], "2025-11-25");
    assert_eq!(result["serverInfo"]["name"], "super-voice");
    assert!(
        result["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("don't speak unasked")),
        "a session is not one the user let ask aloud: {result}"
    );

    let answer = post(
        &via,
        &both,
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::ACCEPTED);
    assert!(answer.body.is_empty(), "{}", answer.body);

    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let answer = post(&via, &both, &list).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(tool_names(&answer), ["speak", "listen"]);

    let speak_only = token(&daemon, &["speak"]).await;
    assert_eq!(tool_names(&post(&via, &speak_only, &list).await), ["speak"]);
    let status_only = token(&daemon, &["status"]).await;
    assert!(tool_names(&post(&via, &status_only, &list).await).is_empty());

    let answer = post(
        &via,
        &both,
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }),
    )
    .await;
    assert_eq!(
        answer.json(),
        json!({ "jsonrpc": "2.0", "id": 3, "result": {} })
    );
}

/// With no model, each tool answers with an error the model sees, over the
/// stream, and `listen` fails before the microphone opens.
#[tokio::test]
async fn a_tool_with_no_model_answers_with_an_error() {
    let daemon = start("mcp-no-model", None).await;
    let socket = daemon.socket().to_path_buf();
    let via = Via::Socket(&socket);
    let both = token(&daemon, &["speak", "transcribe", "status"]).await;

    let answer = post(
        &via,
        &both,
        &json!({
            "jsonrpc": "2.0", "id": "s", "method": "tools/call",
            "params": { "name": "speak", "arguments": { "text": "Hello." } },
        }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let message = answer.message();
    assert_eq!(message["id"], "s");
    assert_eq!(message["result"]["isError"], true, "{message}");

    let answer = post(
        &via,
        &both,
        &json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "listen", "arguments": {} },
        }),
    )
    .await;
    let message = answer.message();
    assert_eq!(message["id"], 4);
    assert_eq!(message["result"]["isError"], true, "{message}");
    assert!(
        message["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("speech to text model")),
        "{message}"
    );
    let status = http_client::status(socket.clone(), &both)
        .await
        .expect("status");
    assert!(!status.stt.busy, "nothing is recording: {status:?}");
}

/// What is not one MCP message is answered as MCP says.
#[tokio::test]
async fn what_is_not_a_message_is_refused() {
    let daemon = start("mcp-refused", None).await;
    let socket = daemon.socket().to_path_buf();
    let via = Via::Socket(&socket);
    let both = token(&daemon, &["speak", "transcribe"]).await;

    let answer = send(&via, Method::POST, Some(&both), &[], "{not json").await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.json()["error"]["code"], -32700);

    let answer = post(
        &via,
        &both,
        &json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }]),
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.json()["error"]["code"], -32600);

    let answer = post(
        &via,
        &both,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["error"]["code"], -32601);

    let answer = send(
        &via,
        Method::POST,
        Some(&both),
        &[("mcp-protocol-version", "1999-01-01")],
        &initialize().to_string(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);

    let answer = send(&via, Method::GET, Some(&both), &[], "").await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);

    let answer = send(&via, Method::POST, None, &[], &initialize().to_string()).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);

    // A cancel for a call that is not running is still a notification.
    let answer = post(
        &via,
        &both,
        &json!({
            "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": 99 },
        }),
    )
    .await;
    assert_eq!(answer.status, StatusCode::ACCEPTED);
}

/// A port nothing listens on, for the daemon's TCP listener.
fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

/// Over TCP an assistant gets in with its key, and is told what the user
/// chose for it; a page, a session token, and a key revoked do not get in.
#[tokio::test]
async fn an_assistant_gets_in_over_tcp_with_its_key_alone() {
    let port = free_port();
    let daemon = start("mcp-tcp", Some(port)).await;
    let settings = token(&daemon, &["settings", "speak", "transcribe"]).await;
    let via = Via::Tcp(port);

    let (status, list) = daemon
        .request(Method::GET, "/assistants", Some(&settings), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list["address"],
        format!("http://127.0.0.1:{port}/v1/mcp"),
        "the daemon took the test's port"
    );
    let (status, _) = daemon
        .request(
            Method::POST,
            "/assistants/codex/connect",
            Some(&settings),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = daemon
        .request(Method::GET, "/assistants/codex/key", Some(&settings), None)
        .await;
    let key = body["key"].as_str().expect("key").to_string();

    let answer = post(&via, &key, &initialize()).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let instructions = answer.json()["result"]["instructions"].clone();
    assert!(
        instructions
            .as_str()
            .is_some_and(|text| text.contains("don't speak unasked")),
        "{instructions}"
    );
    // Until the user lets it ask its questions aloud, it can't listen.
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    assert_eq!(tool_names(&post(&via, &key, &list).await), ["speak"]);
    let listen = json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "listen", "arguments": { "question": "Which one?" } },
    });
    let refused = post(&via, &key, &listen).await.json();
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(
        refused["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Ask questions aloud")),
        "{refused}"
    );

    let (status, _) = daemon
        .request(
            Method::PATCH,
            "/assistants/codex",
            Some(&settings),
            Some(&json!({ "ask_questions_aloud": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let answer = post(&via, &key, &initialize()).await;
    assert!(
        answer.json()["result"]["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("ask them questions aloud")),
        "{}",
        answer.body
    );
    assert_eq!(
        tool_names(&post(&via, &key, &list).await),
        ["speak", "listen"]
    );

    // A key made by hand has no such switch: its client may listen, and is
    // not told to ask on its own.
    let (status, made) = daemon
        .request(
            Method::POST,
            "/assistants/keys",
            Some(&settings),
            Some(&json!({ "name": "My editor" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let by_hand = made["key"].as_str().expect("key").to_string();
    assert_eq!(
        tool_names(&post(&via, &by_hand, &list).await),
        ["speak", "listen"]
    );
    let answer = post(&via, &by_hand, &initialize()).await;
    assert!(
        answer.json()["result"]["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("don't speak unasked")),
        "{}",
        answer.body
    );

    let page = send(
        &via,
        Method::POST,
        Some(&key),
        &[("origin", "http://127.0.0.1:8910")],
        &initialize().to_string(),
    )
    .await;
    assert_eq!(page.status, StatusCode::FORBIDDEN, "{}", page.body);
    assert_eq!(page.json()["data"]["reason"], "browser_not_allowed");

    let session = post(&via, &settings, &initialize()).await;
    assert_eq!(session.status, StatusCode::UNAUTHORIZED, "{}", session.body);

    let (status, _) = daemon
        .request(Method::DELETE, "/assistants/codex", Some(&settings), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let revoked = post(&via, &key, &initialize()).await;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED, "{}", revoked.body);
}
