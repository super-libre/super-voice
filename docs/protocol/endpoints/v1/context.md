# `/stt/context`

**Dictation contexts**: what the user is dictating, so a model can hear it.

A context is two things a backend can be told about the speech it is about to
receive, and they are separate fields because they are consumed differently:

- `prompt` — free text, for a model that follows instructions.
- `vocabulary` — a list of terms to bias recognition toward, for a model that
  does not. Most transcription models are this kind. OpenAI's own guide says
  Whisper "doesn't follow instructions like a general-purpose text model", and
  its `prompt` parameter is really a vocabulary hint.

A backend may accept one half, the other, or both. One context serves the whole
pipeline: a "Coding" context carries the vocabulary the transcription stage
needs to hear `main branch` rather than "Maine branch", and the prompt a
post-processing stage uses to clean up like a programmer.

The vocabulary is a **list** rather than one blob of text because every consumer
splits it differently — Deepgram wants one `keyterm=` per term, `whisper-1`
wants them joined — and a term containing whatever delimiter a blob chose, like
`Menjivar, Jorge`, would be ambiguous in one and not the other. The daemon holds
the structure and splits once, so nobody downstream guesses.

Contexts are **not** under `/v1/settings/`. That namespace is one stored value
apiece; this is a set of objects with a selection over it. They are also not
backend options, because a context describes the user rather than the backend —
putting it in a backend's `[[options]]` would mean re-typing the same vocabulary
for every backend installed.

## Which context a backend gets

One context is **active** at a time, and every backend follows it unless it has
been pointed elsewhere. Three states, one per verb on
`/stt/backend/{backend_id}/context`:

| State    | How it is set                                   | What the backend is sent               |
|----------|-------------------------------------------------|----------------------------------------|
| `active` | `DELETE /stt/backend/{backend_id}/context`          | Whatever `GET /stt/context/active` reports |
| `pinned` | `POST /stt/backend/{backend_id}/context` `{"id":"coding"}` | That context, whatever is active |
| `none`   | `POST /stt/backend/{backend_id}/context` `{"id":null}`    | Nothing                          |

A pin to a context that has since been **deleted** resolves to nothing — it does
**not** fall back to the active one. "Use this context" and "use whatever is
active" are different instructions, and silently promoting the first to the
second would send a user's coding vocabulary to a backend they had deliberately
pointed somewhere else. The pin itself is kept, so re-creating the context
restores it.

## A dictation's own context

A request that starts a dictation can bring context for that dictation alone:
the `context` field of [`POST /stt/transcribe`](./transcribe.md), or an
assistant's `listen` call on [the MCP server](../../mcp.md). It is what the app
or the conversation knows about what the user is about to say, like the names
an assistant has just used, and it is added to the user's context, never in
place of it:

1. The prompt is the user's, a blank line, then the dictation's.
2. The vocabulary is the user's terms, then the dictation's that the user's
   does not have, compared without regard to case.
3. The user's context always goes whole. The dictation's is cut to the room
   left within [the limits](#limits): the prompt where it runs out, the terms
   from the end.

It follows the backend's state above: a backend pinned to no context, or a
model that takes none, hears neither. Nothing is stored, and the backend's own
context is unchanged for every other request.

## Auth

- **Required scope:** `settings`.
- `Authorization: Bearer <session_token>` is required.
- Tokens without the `settings` scope get `403 scope_denied`.

## Limits

Contexts are delivered to a backend as request headers, so the ceiling is what a
header can carry. A context that cannot be sent is refused rather than stored,
because storing it would report success and then deliver nothing.

| Field        | Limit                                                        |
|--------------|--------------------------------------------------------------|
| `id`         | 1–64 characters of `a-z`, `0-9`, `-`, `_`. `active` and `list` are reserved. |
| `name`       | Required, non-blank.                                          |
| `prompt`     | 4000 characters.                                              |
| `vocabulary` | 200 terms, 4000 characters in total.                          |

Blank vocabulary terms are dropped and every term is trimmed, rather than
refused: a settings UI editing one input per term keeps an empty row for the
next one, so blanks arrive by design.

Each backend has a tighter limit of its own that the daemon cannot know —
`whisper-1` takes 224 tokens, Deepgram 500 — so a long vocabulary may be
truncated downstream. These limits only stop a runaway list from being stored.

`active` and `list` are reserved because both are paths in this namespace, and a
router prefers the literal: a context taking either id would be stored fine and
then be permanently unreachable, every read of it answering with something else.
A request to store one never reaches the store, so it is refused by the route it
lands on instead: `POST` or `DELETE /stt/context/list` is `405`, as that path
only lists, and `POST /stt/context/active` is the request that
[chooses the active context](#post-sttcontextactive), which refuses any field but
`id` with `400 invalid_value`.

## `GET /stt/context/list`

**Response (200):**

```jsonc
{
  "status": "success",
  "active": "coding",
  "contexts": [
    {
      "id": "coding",
      "name": "Coding",
      "prompt": "I dictate code. Prefer programming terms.",
      "vocabulary": ["main branch", "rebase", "kubectl", "Menjivar"]
    },
    { "id": "email", "name": "Email", "prompt": "", "vocabulary": [] }
  ]
}
```

`contexts` is in the order the user arranged them, which is the order to render.
`active` is `null` when no context is in force.

## `GET /stt/context/{id}`

**Response (200):**

```jsonc
{
  "status": "success",
  "context": {
    "id": "coding",
    "name": "Coding",
    "prompt": "I dictate code. Prefer programming terms.",
    "vocabulary": ["main branch", "rebase"]
  }
}
```

## `POST /stt/context/{id}`

Create or replace. Upsert rather than a create/update pair, because `id` is the
client's to choose — there is no id-minting step for two verbs to straddle, and
splitting them would only make a settings UI know whether the editor it opened
was on a new context or an existing one.

An existing context keeps its place in the list; a new one is appended.

**Request:**

```http
POST /stt/context/coding HTTP/1.1
Host: voice.local
Authorization: Bearer …64hex…
Content-Type: application/json

{
  "name": "Coding",
  "prompt": "I dictate code. Prefer programming terms.",
  "vocabulary": ["main branch", "rebase", ""]
}
```

| Field        | Type     | Required | Notes                                                    |
|--------------|----------|----------|----------------------------------------------------------|
| `name`       | string   | yes      | What the user calls it.                                   |
| `prompt`     | string   | no       | Defaults to empty. A context that is only a vocabulary is a perfectly good context. |
| `vocabulary` | string[] | no       | Defaults to empty. Blank entries are dropped, each term trimmed. |

**Response (200)** — the context as stored, so the blank term above is gone:

```jsonc
{
  "status": "success",
  "context": {
    "id": "coding",
    "name": "Coding",
    "prompt": "I dictate code. Prefer programming terms.",
    "vocabulary": ["main branch", "rebase"]
  }
}
```

A running backend picks this up on its **next request**. There is no model
reload to do: a context is delivered as headers, so changing one is a matter of
changing what the running instance injects. A realtime session reads its headers
once when it opens, so an edit made mid-recording applies to the next one.

## `DELETE /stt/context/{id}`

Removes the context. If it was active, nothing is active afterwards. A backend
pinned to it keeps its pin and resolves to nothing until the id exists again.

**Response (200):**

```jsonc
{ "status": "success", "message": "Context coding deleted" }
```

## `GET /stt/context/active`

**Response (200):**

```jsonc
{
  "status": "success",
  "id": "coding",
  "context": { "id": "coding", "name": "Coding", "prompt": "…", "vocabulary": ["…"] }
}
```

Both the id and the object, so a picker can render the current choice in one
request. `id` and `context` are both `null` when nothing is active.

## `POST /stt/context/active`

**Request:**

```jsonc
{ "id": "coding" }
```

| Field | Type          | Required | Notes                                       |
|-------|---------------|----------|---------------------------------------------|
| `id`  | string / null | no       | `null` or absent leaves no context active.  |

`id` is the only field. Any other is `400 invalid_value`, its `message` naming
the field: this path is also where a context with the id `active` would be
stored, and a body meant for that must not be read as "leave none active".

**Response (200):** the same shape as `GET /stt/context/active`.

## `GET /stt/backend/{backend_id}/context`

`{backend_id}` is the backend's `source` as [`GET /backend/list`](./backends.md)
reports it, percent-encoded.

**Response (200):**

```jsonc
{
  "status": "success",
  "mode": "pinned",
  "id": "email",
  "context": { "id": "email", "name": "Email", "prompt": "…", "vocabulary": [] }
}
```

| Field     | Type          | Notes                                                                    |
|-----------|---------------|--------------------------------------------------------------------------|
| `mode`    | string        | `active`, `pinned` or `none` — see the table at the top.                   |
| `id`      | string / null | The pinned id. Only meaningful when `mode` is `pinned`.                    |
| `context` | object / null | What this backend will actually be sent, already resolved. Read this rather than re-deriving it. |

## `POST /stt/backend/{backend_id}/context`

**Request:**

```jsonc
{ "id": "email" }
```

| Field | Type          | Required | Notes                                                                 |
|-------|---------------|----------|-----------------------------------------------------------------------|
| `id`  | string / null | no       | The context to pin to. `null` sends this backend no context at all; `DELETE` is how it goes back to following the active one. |

**Response (200):** the same shape as the `GET`.

## `DELETE /stt/backend/{backend_id}/context`

Clears the override, so the backend follows whichever context is active.

Distinct from `POST {"id": null}`, which pins it to *no* context and keeps it
there while the active one changes around it.

**Response (200):** the same shape as the `GET`, with `mode` now `active`.

## Events

Every write here publishes `daemon_status_changed` with
`setting: "contexts"` on [`/events`](./events.md). Contexts are global, so a
second settings window showing the list needs to hear that another app edited
it. One topic for the whole family: the answer to any of these writes is the
same — re-read the list.

## Errors

| HTTP | `error_code`      | Meaning                                                                 |
|------|-------------------|--------------------------------------------------------------------------|
| 400  | `invalid_value`   | The id is not a slug, the name is blank, or the prompt or vocabulary is longer than can be delivered; or `POST /stt/context/active` carried a field other than `id`. The `message` says which. |
| 401  | `invalid_session` | Token unknown / expired / `exe_changed`.                                  |
| 403  | `scope_denied`    | Token lacks the `settings` scope.                                         |
| 404  | `not_found`       | No context has that id, whether read, written or deleted.                 |
| 404  | `unknown_backend` | No installed backend has that `source`.                                   |
