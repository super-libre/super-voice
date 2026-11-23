# `/assistants`

The AI assistants Super Voice connects to, so they can speak to the user and
hear them through Super Voice's MCP server ([`/mcp`](../../mcp.md)): where each
one stands, connecting one, disconnecting one, and whether Super Voice says
aloud when it needs the user. Also the keys made by hand, for any other MCP
client.

The daemon owns them, as it owns the [keyboard shortcuts](./shortcuts.md). It
makes each assistant a key of its own and writes Super Voice into the
assistant's config, so every client sees the same answer, and a client that
cannot write another program's files, like a sandboxed app, can still connect
one.

| `id`          | Assistant   | How Super Voice gets in                                                                 | How it gets its key                          |
|---------------|-------------|-----------------------------------------------------------------------------------------|----------------------------------------------|
| `claude-code` | Claude Code | The daemon installs Super Voice's plugin, with `claude plugin install`.                 | Set as the plugin's secret option `key`, with `claude plugin configure --values-stdin`, which Claude Code keeps with its credentials. |
| `codex`       | Codex       | An entry in `$CODEX_HOME/config.toml`, or `~/.codex/config.toml`, and Super Voice's plugin, from a marketplace the daemon writes to `~/.local/share/super-voice/assistants/codex`, with `codex plugin add`. Codex runs the plugin's hooks once the user trusts them in `/hooks`. | Written into the entry, as its `http_headers`. |
| `opencode`    | OpenCode    | An entry in its global `opencode.json`, under `$XDG_CONFIG_HOME/opencode`, or `~/.config/opencode`, and Super Voice's plugin, written to `plugins/super-voice` beside it, which OpenCode loads by itself. | Written into the entry, which the plugin reads too. |
| `antigravity` | Antigravity CLI (`agy`) | An entry in `~/.gemini/config/mcp_config.json`, which the Antigravity App and IDE read too, and Super Voice's plugin, written to `~/.local/share/super-voice/assistants/antigravity` and installed with `agy plugin install`. Its hooks run `voice assistant-hook antigravity <event>`. | Written into the entry. The hooks get it from `voice`. |

The entry is named `super-voice` in every file, and everything else in the file
is kept. `OpenCode`'s file is edited in place, so its comments and trailing
commas stay. Any other JSON file with them is refused rather than rewritten,
since rewriting it would lose them.

Each plugin's manifest carries Super Voice's own version, so an assistant that
keeps a plugin per version takes a new release's. Disconnecting uninstalls the
plugin too.

Each assistant's `state`:

| `state`     | Meaning                                                                                       |
|-------------|-----------------------------------------------------------------------------------------------|
| `not_found` | The daemon finds neither its program nor its config. It cannot be connected.                  |
| `found`     | It is here, and Super Voice is not in its config with a key of its own. It can be connected.  |
| `connected` | Super Voice is in its config, and the daemon holds its key.                                   |

The daemon looks for an assistant's program on its own `PATH`, then in the
directories assistants install to that a service's `PATH` usually lacks:
`~/.local/bin`, `~/.npm-global/bin`, `~/.bun/bin`, `~/.claude/local`,
`~/.local/share/pnpm`, `~/.volta/bin`, the Node version managers' (asdf's and
mise's shims, fnm's default version, and each of nvm's versions, the newest
first), `/home/linuxbrew/.linuxbrew/bin`, `/opt/homebrew/bin` and
`/usr/local/bin`. Its config is the directory it keeps its config in, which is
there once it has run. Claude Code is found only by its program, which
connecting it runs. For Claude Code, `connected`
also takes Super Voice's plugin listed under `enabledPlugins` in Claude Code's
user settings, `~/.claude/settings.json`, or the one in `$CLAUDE_CONFIG_DIR`.

The daemon reads the configs on every request, so an entry changed by hand
shows here at once. An assistant reads its config when it starts, so one
already running is connected or disconnected from its next start.

## Keys

An assistant's key lets it speak and hear the user: the scopes `speak` and
`transcribe`, and no other. It never expires, and it works only on the daemon's
TCP listener, sent as `Authorization: Bearer <key>` with no `Origin` (see
[auth.md](../../auth.md#assistant-keys)). The daemon names an assistant's key by
the assistant's `id`.

A key made by hand has the same scopes, under a name the user gives it. It is
for an MCP client that is not one of the four above, which the user sets up
with the key and `address` themselves.

Disconnecting an assistant or revoking a key made by hand also cancels its
calls in flight on `/mcp`: a `listen` waiting for the user ends then, rather
than at its next request.

## Auth

- **Required scope:** `settings`.
- `Authorization: Bearer <session_token>` is required.
- Tokens without the `settings` scope get `403 scope_denied`.
- The routes that make a key or show one, `POST /assistants/{id}/connect`,
  `POST /assistants/keys` and `GET /assistants/{id}/key`, are for Super Voice's
  own clients alone: the settings app, the applet and the CLI, installed beside
  the daemon. Any other caller gets `403 auth_denied` with reason
  `not_official_client`, whatever scopes it holds. An assistant must not be
  able to make itself another key, and neither must any other program the user
  once let in.

## `GET /assistants`

```http
GET /assistants HTTP/1.1
Host: voice.local
Authorization: Bearer …64hex…
```

**Response (200):**

```json
{
  "assistants": [
    {
      "id": "claude-code", "state": "connected", "announce_when_needed": false, "conversation_context": true, "show_talk": true,
      "sessions": [
        {
          "id": "4f1c…", "folder": "/home/me/super-voice", "agents": 0,
          "speaking": true, "listening": false,
          "started_at": "2026-11-20T09:15:00Z", "last_used_at": "2026-11-20T09:42:10Z"
        }
      ]
    },
    { "id": "codex",       "state": "found",     "announce_when_needed": true, "conversation_context": false, "show_talk": true, "sessions": [] },
    { "id": "opencode",    "state": "not_found", "announce_when_needed": true, "conversation_context": true, "show_talk": false, "sessions": [] },
    { "id": "antigravity", "state": "not_found", "announce_when_needed": true, "conversation_context": true, "show_talk": true, "sessions": [] }
  ],
  "custom_keys": [
    { "id": "0f3c9a1e", "name": "My editor", "created_at": "2026-11-19T20:15:00Z", "sessions": [] }
  ],
  "address": "http://127.0.0.1:7300/v1/mcp"
}
```

`address` is where an MCP client reaches Super Voice: `/v1/mcp` on the daemon's
TCP listener, on the port its config names. The listener has to be on, which it
is unless the config turns it off.

`conversation_context` says whether the assistant's `listen` calls are heard
with the context they bring from the conversation, on top of the user's own;
see [the MCP server](../../mcp.md#context-from-the-conversation). On until the
user turns it off.

`show_talk` says whether the assistant's plugin shows Talk above its prompt.
Only Claude Code's and OpenCode's plugins have Talk; the others keep it on and
make nothing of it. On until the user turns it off.

`sessions` are the assistant's, or the key's, open sessions, oldest first: one
conversation each, as [the MCP server tells them apart](../../mcp.md#sessions).
`id` is absent when the client names none; `folder` when it doesn't say;
`agents` counts the agents besides the session's own that have made calls;
`speaking` is whether speech it started is playing. `last_used_at` is when it
last spoke or listened, absent until it does: a call that only checks in, like
its plugin's heartbeat, is not a use.

## `POST /assistants/{id}/connect`

Make the assistant a key of its own, or use the one it has, and write Super
Voice into its config. For Claude Code, the daemon runs, with the `claude` it
found:

```sh
claude plugin marketplace add super-libre/super-voice --scope user
claude plugin install super-voice@super-voice --scope user --config address=<address>
```

`<address>` is the server's, as the list gives it: the plugin's `address`
option, which its MCP entry reaches. A marketplace already added or a plugin
already installed counts as done; installing it again sets the address all
the same. Each
command may run for up to two minutes, since adding the marketplace clones it.

When the daemon's environment sets `SUPER_VOICE_CLAUDE_MARKETPLACE`, it adds
that marketplace instead: anything `claude plugin marketplace add` takes, like
a checkout's directory, to try a plugin that is not on GitHub yet. Its
marketplace must be named `super-voice` too. A plugin from a directory loads in
place, so edits there reach Claude Code at its next start. Adding a marketplace
under a name already taken points that name at the new one, so connecting
again without the variable goes back to GitHub's.

Connecting one that is connected already writes its entry again, which changes
nothing. No request body. Answers the whole list, as `GET /assistants` does.

When the config cannot be changed, or the plugin cannot be installed, a key made
for this request is revoked again.

**Errors:**

| HTTP | `error_code`          | Meaning                                                                         |
|------|-----------------------|---------------------------------------------------------------------------------|
| 403  | `auth_denied`         | Not one of Super Voice's own clients (reason `not_official_client`)             |
| 404  | `not_found`           | No assistant has that id (`unknown_assistant`)                                  |
| 409  | `assistant_not_found` | The daemon finds no sign of the assistant on this computer                      |
| 500  | `internal`            | `could_not_write_assistant_config`, `could_not_change_plugin` or `could_not_save_key`, with the reason |

## `DELETE /assistants/{id}`

Take Super Voice out of the assistant's config, revoke its key, and cancel its
calls in flight. For Claude Code, the daemon runs
`claude plugin uninstall super-voice@super-voice --scope user`, and leaves the
marketplace, which the user may use for more. A plugin not installed counts as
done. Disconnecting one that is not connected changes nothing. Answers the whole
list.

The key is revoked even when the config cannot be changed: the entry left
behind does nothing without it.

**Errors:**

| HTTP | `error_code` | Meaning                                                                    |
|------|--------------|----------------------------------------------------------------------------|
| 404  | `not_found`  | No assistant has that id (`unknown_assistant`)                             |
| 500  | `internal`   | `could_not_write_assistant_config` or `could_not_change_plugin`, and the key is revoked; or `could_not_save_key`: the key could not be revoked, and still works. |

## `PATCH /assistants/{id}`

Change the assistant's switches, one or more. Each is on for every assistant
until the user turns it off, and kept in the daemon's config. Answers the whole
list.

1. `announce_when_needed`: whether Super Voice says aloud when the assistant
   needs the user, like Claude Code's own notices that it needs a permission
   or an answer. Claude Code, Codex and OpenCode announce, through their
   plugins; the Antigravity CLI has no hook for it.
2. `conversation_context`: whether its `listen` calls are heard with the
   context they bring from the conversation.
3. `show_talk`: whether its plugin shows Talk above the prompt, for Claude
   Code and OpenCode. Off, the plugin shows nothing there, the `0` key types a
   `0`, and the microphone does not open after a reply is read.

```json
{ "announce_when_needed": false, "conversation_context": true, "show_talk": false }
```

**Errors:**

| HTTP | `error_code`    | Meaning                                         |
|------|-----------------|-------------------------------------------------|
| 400  | `invalid_value` | The body names no switch, or one is not a boolean |
| 404  | `not_found`     | No assistant has that id (`unknown_assistant`)  |

## `POST /assistants/keys`

Make a key by hand, for an MCP client that is not one of the assistants above.

```json
{ "name": "My editor" }
```

**Response (200):**

```json
{
  "id": "0f3c9a1e",
  "name": "My editor",
  "key": "sva_…64hex…",
  "address": "http://127.0.0.1:7300/v1/mcp"
}
```

This is the only time the key is shown. The list names it by `id` and `name`
alone. The name is trimmed.

**Errors:**

| HTTP | `error_code`    | Meaning                                                              |
|------|-----------------|----------------------------------------------------------------------|
| 400  | `invalid_value` | The name is blank (`name_required`) or an assistant's id (`name_reserved`) |
| 403  | `auth_denied`   | Not one of Super Voice's own clients (reason `not_official_client`)  |
| 500  | `internal`      | The key could not be saved (`could_not_save_key`)                    |

## `DELETE /assistants/keys/{id}`

Revoke a key made by hand, and cancel its calls in flight. It stops working at
once. An assistant's own key is not one of these: disconnecting the assistant
revokes it. Answers the whole list.

**Errors:**

| HTTP | `error_code` | Meaning                                         |
|------|--------------|-------------------------------------------------|
| 404  | `not_found`  | No key made by hand has that id (`unknown_key`) |
| 500  | `internal`   | The key could not be revoked, and still works (`could_not_save_key`) |

## `GET /assistants/{id}/key`

The key the daemon made for the assistant when it was connected, and the
address it reaches the MCP server at with it. The key is what
`voice assistant-header <id>` prints, as
`{"Authorization":"Bearer <key>"}`, for an assistant that gets its key by
running that command. `voice assistant-hook` calls the MCP server with both,
for an assistant whose hooks can only run a command.

**Response (200):**

```json
{ "key": "sva_…64hex…", "address": "http://127.0.0.1:7300/v1/mcp" }
```

**Errors:**

| HTTP | `error_code`  | Meaning                                                             |
|------|---------------|---------------------------------------------------------------------|
| 403  | `auth_denied` | Not one of Super Voice's own clients (reason `not_official_client`) |
| 404  | `not_found`   | No assistant has that id (`unknown_assistant`), or it is not connected (`not_connected`) |

## Errors on every route

| HTTP | `message`         | Meaning                                  |
|------|-------------------|------------------------------------------|
| 401  | `invalid_session` | Token unknown / expired / `exe_changed`  |
| 403  | `scope_denied`    | Token lacks the `settings` scope         |
| 429  | `rate_limited`    | Too many requests; back off              |
