# `/v1/mcp`: Super Voice's MCP server

The daemon serves the [Model Context Protocol](https://modelcontextprotocol.io)
at `/v1/mcp`, so an AI assistant such as Claude Code, Codex, OpenCode or the
Antigravity CLI can speak to the user and hear them. It is one server for every assistant,
on the daemon's TCP listener: `http://127.0.0.1:7300/v1/mcp`, or the port
`http.tcp.port` names in `daemon.ron`.

The user connects an assistant on the Assistants page of the app,
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
| `speak`  | Says `text` aloud, in the voice the user chose. Returns once the speech is queued and playing. Waits first for the session's own speech to finish playing, and isn't said if that speech is stopped while it waits. | `speak`        |
| `listen` | Records the user until they stop talking, and returns what they said. With a `question`, asks it aloud first, and starts recording once it has finished playing. With a `context`, hears them with it. Waits first for the session's own speech to finish playing, so it never cuts off what the assistant just said. | `transcribe`, and `speak` for a question |

`tools/list` lists only the tools the caller's key or token may use, and a tool
call names only one of those: any other is the JSON-RPC error `-32602`. A key
the daemon makes has both scopes.

Both tools carry MCP's annotations `destructiveHint: false` and
`openWorldHint: false`: they play or record audio on this machine, and change
nothing else. A client may run them without asking the user first, as Codex
does, so a spoken answer doesn't wait on an approval.

`listen` records with the stop mode "silence or manual": it ends when the user
stops talking, and the user's own shortcut stops it early. It never toggles a
recording someone else started: when one is running, the call answers with an
error. It types nothing.

### Context from the conversation

`listen` takes an optional `context`, `{"prompt": text, "vocabulary": [text]}`:
what the conversation tells about what the user is about to say, a one-line
topic and the names, files and terms they may use. The daemon adds it to the
user's own context for that take, as it adds
[a request's own context](endpoints/v1/context.md#a-dictations-own-context),
and the `question`, when there is one, goes at the front of its prompt as "The
user is answering: …". Where `POST /stt/transcribe` refuses a context past the
limits, `listen` cuts it to fit: a model's call never fails on its context.

The user decides per assistant, with **Use the conversation as context** on the
Assistants page (`conversation_context` on
[`PATCH /assistants/{id}`](endpoints/v1/assistants.md)). It is on until they
turn it off. Off, a `listen` is heard with the user's context alone, as if it
brought none. A key made by hand has no switch: its client chose to send it.

The daemon logs each `listen` at info as it arrives, before anything records:
the assistant, its session (by name, as the Assistants page shows it, and
id), whether the model or the user asked, and the `question`, the `context`'s
`prompt` and its `vocabulary` whole, as the client sent them, one line each.
It never logs a key or a token.

A tool that cannot do its work answers with a result whose `isError` is `true`
and whose text says why, so the model can tell the user: no model loaded, a
recording in the way, and so on. Silence is not an error: `listen` answers
`(no speech detected)`.

### The plugin's tools

Every assistant Super Voice connects also gets five tools its plugin calls
itself, never the model. For Codex each is marked
`"_meta": {"ui": {"visibility": ["app"]}}`, which Codex reads to keep it from
its model. For the others they go unmarked, and the plugin hides them from the
model itself: Claude Code reads the mark too, and then refuses its plugin's own
calls to the tool. A key made by hand doesn't get them.

| Tool       | What it does                                                                                     |
|------------|--------------------------------------------------------------------------------------------------|
| `stop`     | Stops this session's own speech, even one still waiting for the model, and ends its own recording, which is still transcribed. Another session's are left alone. |
| `status`   | Whether this session's own speech plays or is on its way, and whether it records, whether the user shows the plugin's Talk (the assistant's `show_talk`), and whether the plugin sends a prompt dictated into it by itself (`autosend_dictated_prompts`): `{"speaking": bool, "listening": bool, "talk": bool, "autosend": bool}`. |
| `announce` | Says `text` (or a hook's `message`) aloud, unless the user turned off **Announce when it needs you** for the assistant. Then it says nothing, and answers so. |
| `session`  | `{"event": "start"}` or `{"event": "end"}`: the session opened or closed, so it is listed while it is open, checking in at least once a minute. `{"event": "seen"}`: it is there, from a plugin that cannot check in, like a hook's; it is listed until it has been quiet for 30 minutes. |
| `turn`     | `{"event": "prompt", "text", "spoken"?, "heard"?, "again"?}`: the user sent a prompt. `spoken` says whether it was said aloud. Left out, it is spoken when it holds `heard`, the words the plugin's Talk put in the prompt box, or what the user last dictated into a window, within 90 seconds. Either holds: the dictation's words in order, but for about one in four the user corrected by hand, with anything typed around them; or most of the prompt is the dictation cut down, keeping at least half of it. A dictation of three words or fewer must be there whole. In a script written without spaces between words, as Chinese, Japanese and Thai are, each character counts as a word. Answers `{"spoken": bool, "note": text}`, `note` being what the assistant is told beside a spoken prompt. Sent again with `"again": true`, as a hook that runs before each model call of a turn does, the prompt keeps its turn, and `note` is null; the same text sent without it is a new turn. A prompt of `/read`, or of the plugin's skill as agy (`/super-voice:read`) or Codex (`$read`, `$super-voice:read`) names it, alone or with a number from 0 to 9, is answered by Super Voice instead, as a typed turn the assistant never sees: it reads the session's last reply aloud again, or the one that many before it, and plain `/read` while the session's speech plays stops it. Answers `{"spoken": false, "answered": text, "failed": bool}`, `answered` being what it did, or why it could not, and `failed` saying which. `{"event": "reply", "text"}`: the assistant answered. Super Voice keeps the session's last 10 replies for `/read`, and reads a spoken turn's reply aloud. Answers `{"read": bool}`. `{"event": "draft", "text"}`: the prompt box holds `text`, not sent yet. Answers `{"dictated": bool}`: whether it holds what the user dictated with Super Voice's shortcut, by the same rule and within the same 90 seconds as a prompt. A match uses the dictation up, and the plugin sends the box as spoken. No turn changes. |

Once a plugin has said a turn was typed, `speak` and `listen` refuse in it, so
the assistant answers in writing. A session whose plugin says nothing of its
turns is not held to it. A plugin's own call for what the user asked for
themselves, as Talk's `listen` or a button that reads a reply again, takes
`"asked": true` and is not refused. It doesn't wait for the session's speech
either: the user means to talk over it, or to hear the reply now. No tool
declares `asked`, as with `session`.

A tool a hook calls itself takes `"hook": "codex"`, and then answers as Codex's
hooks read a hook's output: the `UserPromptSubmit` JSON with the note beside a
spoken prompt, `{"decision": "block", "reason": text}` for a `$read` prompt,
and nothing for anything else.

## Sessions

The daemon tells apart the sessions of one assistant: one Claude Code session,
one Codex thread. A tool call names its session by what the client says of
itself, first match wins:

1. The argument `session`, `{"id", "agent", "folder"}`. No tool declares
   it, and it is not a tool's own argument. Super Voice's plugins add it to
   every call they make themselves. OpenCode's and the Antigravity CLI's set
   it on the model's calls too, over anything the model put there. Claude
   Code's leaves the model's calls as they are, since auto mode approves a
   call only as the model made it, and refuses one that fills `session`. The
   daemon names the model's calls by their connection, item 3.
2. `_meta`: Claude Code's `claudecode/sessionId` and `claudecode/agentId`;
   Codex's `sessionId` (the conversation) and `threadId` (an agent's thread,
   when it differs), with the folder from the first git root in
   `x-codex-turn-metadata`; OpenCode's `ai.opencode/sessionID`.
3. The connection: the `Mcp-Session-Id` the daemon gives each connection at
   `initialize`, which clients before the 2026-07-28 revision send back. A
   call that names its session ties its connection to it, so a later call
   over the same connection that names nothing is that session's too; a
   connection nothing named is a session of its own.
4. Otherwise all the caller's calls are one session.

A session is listed from its first call, or from its plugin's `session`
start, until its plugin says it ended, its assistant is disconnected, or it
goes quiet: 3 minutes for one its plugin opened, which checks in at least once a
minute, and 30 minutes for any other. One still recording stays listed. Off
the list, a session that went quiet is remembered for a day: its turn,
spoken or typed, and its replies, so a long task that said nothing to Super
Voice ends in the turn it began. The speech and the
recording a session starts are its own: `stop` and `status` see only those.
[`GET /assistants`](endpoints/v1/assistants.md) lists each assistant's open
sessions.

## Instructions

`initialize` answers with instructions for the model: what the tools do, to give
a question to `listen` rather than call `speak` first, and to use them only
while the user is talking to it by voice. A prompt the user typed gets a
written answer, even one that asks it to speak.

Where Super Voice has a plugin, it holds the model to this: the plugin says
whether each prompt was spoken, with the `turn` tool, and `speak` and `listen`
refuse in a typed turn. An assistant connected without one, as with a key made
by hand, is only told.

## Transport

[Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
with every message a `POST` of its own. The server sends an `Mcp-Session-Id`
with its answer to `initialize`, and uses the one a client sends back only to
tell its sessions apart (see [Sessions](#sessions)) and to match a cancel to
its call. It never refuses a request for a missing or unknown one, so a
client whose id is from before a restart goes on as it was. Between
messages it keeps the tool calls in flight and each session's turn and
replies, in memory: a restart forgets them.

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

The versions it speaks are `2025-11-25` and `2025-06-18`. It does not speak
`2025-03-26`, whose servers take batches of messages.
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

A call whose request names a `progressToken` in `params._meta` gets each
keepalive as a `message` event instead: a `notifications/progress` under that
token, its `progress` counting the keepalives from 1. A client counts that as
the tool's progress, where it may not count a comment: Claude Code stops a
call that sends no response or progress for five minutes.

```
event: message
data: {"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":3,"progress":1}}

```

The response headers are `content-type: text/event-stream`, `cache-control:
no-store` and `x-accel-buffering: no`.

## Cancelling a call

A call ends without an answer, and its stream ends with no `message` event,
when:

1. The client sends `notifications/cancelled` naming the call's request id.
   Sent with the connection's `Mcp-Session-Id`, it reaches that connection's
   call only: two sessions of one assistant each count their own ids. Sent
   without one, it reaches every call the assistant has running under that
   id.
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
