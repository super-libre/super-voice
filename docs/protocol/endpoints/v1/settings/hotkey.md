# `/settings/hotkey`

Read, set and reset the keys the macOS shortcut listener listens for. The
listener (`voice hotkey`, the app bundle's second LaunchAgent) holds one
global shortcut that starts and stops dictation, as `voice record --write`
does. It reads this setting when it starts, and again whenever a
`settings_changed` event names `hotkey` (see [`/events`](../events.md)), and
swaps the shortcut it holds at once.

Linux has no listener: there the desktop binds the command, through
[`/shortcuts`](../shortcuts.md). The setting is stored on every platform all
the same.

## The binding

Modifiers and one key joined by `+`, modifiers first, in any case:

- modifiers: `ctrl` (`control`), `alt` (`option`), `shift`, `cmd`
  (`command`, `super`, `meta`);
- the key: a `keyboard-types` `Code` name, such as `Space`, `KeyR`,
  `Digit1`, `F5`, `ArrowUp`, `Comma`, or its short form (`r`, `1`, `up`,
  `,`).

This is the grammar of `global-hotkey`'s `HotKey::from_str`, which the
listener parses with. On top of it the daemon refuses what the listener
could not register on a Mac, or should not:

- a key a Mac has no key code for (`BrowserBack`, `F21` to `F35`);
- a media key (`MediaPlayPause`), which needs Input Monitoring;
- a key with no modifier, unless it is an F key: a bare letter would stop
  that letter being typed anywhere.

The default is `ctrl+alt+space` (⌃⌥Space). A binding is stored written one
way: modifiers in the Mac's order (`ctrl`, `alt`, `shift`, `cmd`), then the
key's `Code` name, as in `shift+cmd+KeyR`. Setting the default, however it
is spelled, stores no binding, as `DELETE` does.

## Auth

- **Required scope:** `settings`.
- `Authorization: Bearer <session_token>` is required.
- Tokens without the `settings` scope get `403 scope_denied`.

## `GET /settings/hotkey`

```http
GET /settings/hotkey HTTP/1.1
Host: voice.local
Authorization: Bearer …64hex…
```

**Response (200):**

```json
{ "status": "success", "hotkey": "ctrl+alt+space" }
```

`hotkey` is the binding set, or the default when none is. It is always
present.

## `POST /settings/hotkey`

```http
POST /settings/hotkey HTTP/1.1
Host: voice.local
Authorization: Bearer …64hex…
Content-Type: application/json

{ "hotkey": "cmd+shift+r" }
```

| Field    | Type   | Required | Notes                         |
|----------|--------|----------|-------------------------------|
| `hotkey` | string | yes      | A binding, as described above |

**Response (200):** the binding as stored.

```json
{ "status": "success", "hotkey": "shift+cmd+KeyR" }
```

Announced with `settings_changed`, `setting: "hotkey"`, `feature: "stt"`.

## `DELETE /settings/hotkey`

Back to the default.

```json
{ "status": "success", "hotkey": "ctrl+alt+space" }
```

Announced as `POST` is.

**Errors (`POST`):**

| HTTP | `error_code`    | Meaning                                                             |
|------|-----------------|---------------------------------------------------------------------|
| 400  | `invalid_value` | Not a binding, or one the listener could not use; `message` says why |

**Errors (all methods):**

| HTTP | `message`         | Meaning                                 |
|------|-------------------|-----------------------------------------|
| 401  | `invalid_session` | Token unknown / expired / `exe_changed` |
| 403  | `scope_denied`    | Token lacks the `settings` scope        |
