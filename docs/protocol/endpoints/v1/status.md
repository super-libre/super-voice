# `GET /status`

Snapshot of the daemon's current operational state, one part per feature:
which model each has loaded, which device it runs on, and whether it is at
work. Subscriber introspection and other operator info are not exposed here;
for those, the `settings` scope's [`GET /stt/pipeline/1`](./pipeline/stage.md),
[`GET /tts/pipeline/1`](./tts/pipeline/stage.md) and their `device`
endpoints apply.

## Auth

- **Required scope:** `status`.
- `Authorization: Bearer <session_token>` is required.
- Tokens without the `status` scope get `403 scope_denied`.

## `GET /status`

**Request:**

```http
GET /status HTTP/1.1
Host: voice.local
Authorization: Bearer …64hex…
```

**Response (200):**

```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "status": "success",
  "stt": {
    "device":         "cuda",
    "model_loaded":   true,
    "current_model":  "whisper-tiny",
    "selected_model": "whisper-tiny",
    "model_state":    "loaded",
    "busy":           false,
    "can_type":       true
  },
  "tts": {
    "device":         "unknown",
    "model_loaded":   false,
    "selected_model": "kokoro-82m",
    "model_state":    "idle",
    "busy":           false
  }
}
```

`stt` is speech to text's part and `tts` text to speech's. Both are always
present, with the same fields (`can_type` is speech to text's alone):

| Field           | Type    | Notes                                                                                  |
|-----------------|---------|----------------------------------------------------------------------------------------|
| `device`        | string  | The accelerator the feature's loaded model is actually running on: `"cpu"`, `"cuda"`, `"rocm"`, `"metal"`, `"vulkan"`, or `"remote"` for an online model. `"unknown"` if nothing is loaded. |
| `model_loaded`  | bool    | `false` while the feature's model is still loading, after a failed switch, or when none is selected |
| `current_model` | string? | The loaded model's name (e.g. `whisper-tiny`); absent when `model_loaded` is `false`   |
| `selected_model` | string? | The model selected in the feature's first stage, loaded or not; absent when none is selected. With [idle unload](./settings/idle_unload.md) on, a selected model is out of memory most of the time, which `current_model` alone cannot tell from no model at all. Absent from a daemon older than the field. |
| `model_state`   | string? | Where `selected_model` stands, as [`GET /stt/pipeline/1/model`](./pipeline/model.md) and [`GET /tts/pipeline/1/model`](./tts/pipeline/model.md) report it: `loaded`, `idle` (out of memory, and its next use loads it) or `not_loaded`, which is also what it says with nothing selected. Absent from a daemon older than the field. |
| `busy`          | bool    | Whether the feature is at work right now. For `stt`, `true` while a daemon-mic cycle is active — audio capture, which starts at once even while a model loads on demand, **and** the transcription and typing after it, which wait for that load; a toggle hotkey consults it and calls [`POST /stt/transcribe/stop`](./transcribe/stop.md) when `true`, [`POST /stt/transcribe`](./transcribe.md) when `false`. For `tts`, `true` while an utterance is being synthesized or played; a [`POST /tts/speak`](./tts/speak.md) then interrupts it. |
| `can_type`      | bool?   | Speech to text only: whether the daemon may type into other apps. On macOS, whether it holds Device Control (called Accessibility before macOS 27), asked of macOS on every read, so a switch flipped in System Settings shows at the next read. Absent where the daemon can't tell without trying (Linux, where a dictation builds its keyboard), from `tts`, and from a daemon older than the field. A dictation that can't be typed goes to the clipboard, and [`typing_failed`](./events.md#transcription) says so. |

**Errors:**

| HTTP | `message`         | Meaning                                                       |
|------|-------------------|---------------------------------------------------------------|
| 401  | `invalid_session` | Token unknown / expired / `exe_changed` — re-auth and retry   |
| 403  | `scope_denied`    | Token lacks the `status` scope                               |
