# `/v1/mcp`: Super Voice's MCP server

The daemon serves the [Model Context Protocol](https://modelcontextprotocol.io)
at `/v1/mcp`, so an AI assistant such as Claude Code, Codex, OpenCode or Gemini
CLI can speak to the user and hear them. It is one server for every assistant,
on the daemon's TCP listener: `http://127.0.0.1:7300/v1/mcp`, or the port
`http.tcp.port` names in `daemon.ron`.

The user connects an assistant on the Assistants page of the settings app,
which asks the daemon ([`/assistants`](./endpoints/v1/assistants.md)) to make
the assistant a key and write Super Voice into its config. An MCP client that is
not one of those assistants gets a key made by hand there, with the address.

MCP is JSON-RPC: the operation is a method in the body, not a path. So
`/v1/mcp` is not in the [OpenAPI document](./README.md#build-a-client), and this
page is its contract.

## Who gets in

Over TCP, an assistant presents its key and nothing else
([auth.md](./auth.md#assistant-keys)):

```http
POST /v1/mcp HTTP/1.1
Host: 127.0.0.1:7300
Authorization: Bearer sva_…64hex…
Content-Type: application/json
Accept: application/json, text/event-stream
```

1. **No `Origin`.** An assistant is a program and never sends one, and a browser
   always does on a request like this, so a request with an `Origin` is a page
   and is refused with `403`, reason `browser_not_allowed`, whatever key it
   holds. That is also how this server checks `Origin`, as MCP asks it to.
2. **A loopback `Host`**: `127.0.0.1`, `localhost` or `[::1]`, on any port. Any
   other is a name pointed at the listener from outside, as DNS rebinding does,
   and is refused with `403`, reason `host_not_allowed`.
3. **An assistant key** as the bearer, in the `Authorization` header. A session
   token is not one, even a valid one: `401 invalid_session`.

The origin allowlist the rest of the TCP listener has does not apply here: it is
for pages, and no page gets in.

Over the Unix socket a client with a session token gets in too, as a test or the
CLI does. An assistant key presented there is refused with `401
invalid_session`, reason `wrong_caller`, and is not revoked.

Each assistant is rate-limited by its key, apart from every other client.

## The tools

| Tool     | What it does                                                                                       | Scope it takes |
|----------|----------------------------------------------------------------------------------------------------|----------------|
| `speak`  | Says `text` aloud, in the voice the user chose. Returns once the speech is queued and playing.    | `speak`        |
| `listen` | Records the user until they stop talking, and returns what they said. With a `question`, asks it aloud first, and starts recording once it has finished playing. | `transcribe`, and `speak` for a question |

`tools/list` lists only the tools the caller's key or token may use, and a tool
call names only one of those: any other is the JSON-RPC error `-32602`. A key
the daemon makes has both scopes, so it gets both tools.

`listen` records with the stop mode "silence or manual": it ends when the user
stops talking, and the user's own shortcut stops it early. It never toggles a
recording someone else started: when one is running, the call answers with an
error. It types nothing.

A tool that cannot do its work answers with a result whose `isError` is `true`
and whose text says why, so the model can tell the user: no model loaded, a
recording in the way, and so on. Silence is not an error: `listen` answers
`(no speech detected)`.

## Instructions

`initialize` answers with instructions for the model: what the tools do, to give
a question to `listen` rather than call `speak` first, and when to speak. By
default that is only when the user asks to talk. When the user turns on **Ask
questions aloud** for the assistant (`PATCH /assistants/{id}`), the model is
told it may also ask the user its questions aloud, with `listen`. A key made by
hand, and a session token, never are. An assistant is told this when it starts
a session, so a change reaches one already running from its next session.

## Transport

[Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
without sessions: every message is a `POST` of its own, the server sends no
`Mcp-Session-Id`, and nothing is kept between messages but the tool calls in
flight.

| Message                                   | Answer                                                     |
|-------------------------------------------|------------------------------------------------------------|
| `initialize`, `ping`, `tools/list`        | `200` `application/json`, the JSON-RPC response            |
| `tools/call`                              | `200` `text/event-stream`, see below                       |
| A notification, such as `notifications/initialized` | `202`, no body                                   |
| A JSON-RPC response                       | `202`, no body. This server sends no requests to answer.   |
| An unknown method                         | `200` `application/json`, the JSON-RPC error `-32601`     |
| A body that is not JSON                   | `400`, the JSON-RPC error `-32700`, with `id` `null`      |
| JSON that is not one message, a batch included | `400`, the JSON-RPC error `-32600`                   |

`GET` and any other method but `POST` are `405`: the server opens no stream of
its own. A request whose `MCP-Protocol-Version` header names a version this
server does not speak is `400`.

The versions it speaks are `2025-11-25`, `2025-06-18` and `2025-03-26`.
`initialize` agrees on the one the client asks for, or offers `2025-11-25` when
it asks for another.

### A tool call's stream

A tool call can take minutes: `listen` waits for the user. Its answer is a
stream, which starts at once and stays busy:

```
: stream-open

: keepalive

: keepalive

event: message
data: {"jsonrpc":"2.0","id":7,"result":{"content":[{"type":"text","text":"Yes, the second one."}],"isError":false}}

```

1. `: stream-open`, a comment, as soon as the call starts, so the client sees
   the response begin.
2. `: keepalive`, a comment, every two seconds while the tool runs, so neither
   the connection nor a timer the client keeps on it runs out.
3. One `message` event with the JSON-RPC response, and the stream ends.

The response headers are `content-type: text/event-stream`, `cache-control:
no-store` and `x-accel-buffering: no`.

## Cancelling a call

A call ends without an answer, and its stream ends with no `message` event,
when:

1. The client sends `notifications/cancelled` naming the call's request id.
   The server cannot tell two sessions of one assistant apart, so a cancel
   reaches every call that assistant has running under that id.
2. The client closes the connection. The server notices at the next keepalive,
   within two seconds.
3. The user disconnects the assistant, or revokes the key made by hand. The key
   stops working for new requests at once, and its calls running are cancelled.

What cancelling does depends on where the call is:

1. `speak` goes on speaking, as `POST /v1/tts/speak` does when its client hangs
   up. Only the wait for it ends. `POST /v1/tts/speak/stop`, or the user's own
   shortcut, stops it.
2. `listen` asking its question stops the question, if it is still the one
   speaking. Speech started since is left alone.
3. `listen` recording stops the recording. The take still finishes, so the
   microphone is freed as at any other stop, and its text goes nowhere.

## Example

With a key in `$KEY`:

```bash
curl -s http://127.0.0.1:7300/v1/mcp \
     -H "Authorization: Bearer $KEY" \
     -H 'Content-Type: application/json' \
     -H 'Accept: application/json, text/event-stream' \
     -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"listen","arguments":{"question":"Which one should I open?"}}}'
```
