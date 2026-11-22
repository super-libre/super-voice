// SPDX-License-Identifier: GPL-3.0-only
//! Calling a tool on Super Voice's MCP server as an assistant, with its key:
//! for an assistant whose hooks can only run a command, which then calls
//! Super Voice for it (`voice assistant-hook`).
//!
//! An assistant's key is good on the MCP server's TCP address alone, as the
//! assistant itself uses it, so this goes there rather than to the daemon's
//! socket.

use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::{Value, json};

/// How long a call may take. A hook has its own limit, and Super Voice's
/// plugin tools answer at once.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A tool's answer: its text, and whether it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAnswer {
    pub text: String,
    pub is_error: bool,
}

/// Call `tool` with `arguments` on the MCP server at `address`, such as
/// `http://127.0.0.1:7300/v1/mcp`, sending `key` as the bearer token.
///
/// # Errors
///
/// The reason, when the server can't be reached, refuses the key, or gives no
/// answer to the call.
pub async fn call_tool(
    address: &str,
    key: &str,
    tool: &str,
    arguments: Value,
) -> Result<ToolAnswer, String> {
    tokio::time::timeout(TIMEOUT, call(address, key, tool, arguments))
        .await
        .map_err(|_| {
            format!(
                "Super Voice gave no answer in {} seconds",
                TIMEOUT.as_secs()
            )
        })?
}

async fn call(
    address: &str,
    key: &str,
    tool: &str,
    arguments: Value,
) -> Result<ToolAnswer, String> {
    let uri: hyper::Uri = address
        .parse()
        .map_err(|e| format!("{address} is not an address: {e}"))?;
    let host = uri
        .host()
        .ok_or_else(|| format!("{address} names no host"))?
        .to_string();
    let port = uri.port_u16().unwrap_or(80);
    let stream = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("could not reach Super Voice at {address}: {e}"))?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
            .await
            .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let request = hyper::Request::post(uri.path())
        .header("host", format!("{host}:{port}"))
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Full::new(Bytes::from(body.to_string())))
        .map_err(|e| e.to_string())?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let is_stream = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|e| e.to_string())?
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    if !status.is_success() {
        return Err(format!("Super Voice answered {status}: {}", text.trim()));
    }
    answer_of(&text, is_stream)
}

/// The answer to request 1 in `body`: a JSON-RPC message, or a stream of
/// them.
fn answer_of(body: &str, is_stream: bool) -> Result<ToolAnswer, String> {
    let message = if is_stream {
        body.split("\n\n")
            .filter_map(|event| {
                let data: Vec<&str> = event
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect();
                serde_json::from_str::<Value>(&data.join("\n")).ok()
            })
            .find(|message| message["id"] == 1)
            .ok_or_else(|| "Super Voice gave no answer".to_string())?
    } else {
        serde_json::from_str::<Value>(body).map_err(|e| e.to_string())?
    };
    if let Some(error) = message.get("error") {
        return Err(error["message"].as_str().unwrap_or("error").to_string());
    }
    let result = &message["result"];
    let text = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<String>();
    Ok(ToolAnswer {
        text,
        is_error: result["isError"].as_bool().unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_read_from_json_or_a_stream() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Seen."}],"isError":false}}"#;
        assert_eq!(
            answer_of(json, false),
            Ok(ToolAnswer {
                text: "Seen.".to_string(),
                is_error: false
            })
        );
        let stream = format!(": stream-open\n\nevent: message\ndata: {json}\n\n");
        assert_eq!(answer_of(&stream, true).unwrap().text, "Seen.");
        let refused = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"No."}],"isError":true}}"#;
        assert!(answer_of(refused, false).unwrap().is_error);
        let error = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Unknown tool"}}"#;
        assert_eq!(answer_of(error, false), Err("Unknown tool".to_string()));
        assert!(answer_of(": nothing\n\n", true).is_err());
    }
}
