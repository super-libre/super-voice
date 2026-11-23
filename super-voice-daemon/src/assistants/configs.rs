// SPDX-License-Identifier: GPL-3.0-only
//! Super Voice's entry in AI assistants' own config files: adding it,
//! removing it, and whether it is there.
//!
//! The daemon writes these when the user connects or disconnects an
//! assistant. The entry is named [`ENTRY`] in every file. It points the
//! assistant at the daemon's MCP server and gives it the key made for it.
//! Claude Code is not here: its plugin connects it.
//!
//! 1. Codex: `config.toml`. Codex gets the key from a command it runs,
//!    `voice assistant-header codex`, so the file holds no key.
//! 2. `OpenCode`: `opencode.json`, with the key in a header.
//! 3. Antigravity CLI: `mcp_config.json`, with the key in a header.
//!
//! The transforms are pure functions over a file's text, tested on strings.
//! [`read`] and [`update`] are the file wrappers around them. Everything else
//! in a file is kept: comments and layout in Codex's TOML, and keys and their
//! order in the JSON files. `OpenCode` also reads comments and trailing
//! commas, which a JSON parser would drop, so its file is edited in place
//! when it has them. Any other JSON file with them is refused, not
//! rewritten.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize as _;
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item, Table, value};

/// The name of Super Voice's entry in every assistant's config.
pub const ENTRY: &str = "super-voice";

/// Why a config file was not changed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is not TOML.
    #[error("the file is not valid TOML: {0}")]
    Toml(#[from] toml_edit::TomlError),
    /// The file is not plain JSON. It may have comments or trailing commas,
    /// which rewriting it would lose.
    #[error("the file is not plain JSON, so it is left as it is: {0}")]
    NotPlainJson(#[from] serde_json::Error),
    /// The file is not JSON, even with comments and trailing commas.
    #[error("the file is not JSON: {0}")]
    NotJson(String),
    /// The file is JSON, but not an object.
    #[error("the file is not a JSON object")]
    NotAnObject,
    /// A key on the way to the entry, named by its path, holds something
    /// other than a table.
    #[error("`{0}` in the file is not a table")]
    NotATable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// Codex.
//
// From Codex's source, codex-rs/config/src/mcp_types.rs (openai/codex at
// 7066a55a7b), and its docs, https://learn.chatgpt.com/docs/extend/mcp: a
// Streamable HTTP server is a `[mcp_servers.<name>]` table with `url`.
// `http_headers_helper` is "a local-only shell command that prints a JSON
// object of dynamic HTTP headers", run with `sh -c`, for servers in the
// local environment only. `tool_timeout_sec` is how long a tool may run, 60
// seconds unless set. The file is `$CODEX_HOME/config.toml`, `~/.codex` when
// `CODEX_HOME` is not set (codex-rs/utils/home-dir/src/lib.rs).

/// How long Codex lets one of Super Voice's tools run, in seconds. Codex's
/// own default, 60, cuts off a spoken answer.
pub const CODEX_TOOL_TIMEOUT_SEC: i32 = 600;

/// Keys an entry may have picked up that stop it working as written: a
/// stdio server's, which Codex refuses beside `url`; a bearer token, which
/// wins over the helper's `Authorization`; another environment, where the
/// helper does not run; and `enabled`, since connecting turns it on.
const CODEX_STALE_KEYS: [&str; 9] = [
    "command",
    "args",
    "env",
    "env_vars",
    "cwd",
    "bearer_token",
    "bearer_token_env_var",
    "environment_id",
    "enabled",
];

/// Codex's config file: `config.toml` in `$CODEX_HOME`, or in `~/.codex`
/// when it is not set.
#[must_use]
pub fn codex_file() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;
    Some(home.join("config.toml"))
}

/// The command Codex runs for Super Voice's headers, given the `voice`
/// program: `voice assistant-header codex`, which prints
/// `{"Authorization":"Bearer <key>"}`. Codex runs it with `sh -c`, so the
/// program is quoted for the shell.
#[must_use]
pub fn codex_headers_helper(voice: &str) -> String {
    format!("{} assistant-header codex", shell_quote(voice))
}

/// `word` as `sh` reads it back: as it is when it is plain, in single quotes
/// otherwise.
#[must_use]
pub fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%=,".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// `text`, Codex's config, with Super Voice's entry: the server at `url`,
/// its headers from `headers_helper` (see [`codex_headers_helper`]), and
/// tools allowed [`CODEX_TOOL_TIMEOUT_SEC`]. An entry already there is
/// updated in place, keeping the keys it has of its own, and a longer
/// timeout.
///
/// # Errors
///
/// When `text` is not TOML, or `mcp_servers` or the entry is not a table.
pub fn codex_with_super_voice(
    text: &str,
    url: &str,
    headers_helper: &str,
) -> Result<String, Error> {
    let mut doc: DocumentMut = text.parse()?;
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            // Implicit, so the file gets `[mcp_servers.super-voice]` and no
            // empty `[mcp_servers]` above it.
            let mut servers = Table::new();
            servers.set_implicit(true);
            Item::Table(servers)
        })
        .as_table_like_mut()
        .ok_or_else(|| Error::NotATable("mcp_servers".to_string()))?;
    let added = !servers.contains_key(ENTRY);
    if added {
        let mut entry = Table::new();
        if !text.trim().is_empty() {
            // A blank line from what comes before it.
            entry.decor_mut().set_prefix("\n");
        }
        servers.insert(ENTRY, Item::Table(entry));
    }
    let entry = servers
        .get_mut(ENTRY)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| Error::NotATable(format!("mcp_servers.{ENTRY}")))?;
    for key in CODEX_STALE_KEYS {
        entry.remove(key);
    }
    entry.insert("url", value(url));
    entry.insert("http_headers_helper", value(headers_helper));
    let long_enough = entry.get("tool_timeout_sec").is_some_and(|timeout| {
        timeout
            .as_integer()
            .is_some_and(|secs| secs >= i64::from(CODEX_TOOL_TIMEOUT_SEC))
            || timeout
                .as_float()
                .is_some_and(|secs| secs >= f64::from(CODEX_TOOL_TIMEOUT_SEC))
    });
    if !long_enough {
        entry.insert("tool_timeout_sec", value(i64::from(CODEX_TOOL_TIMEOUT_SEC)));
    }
    if added
        && let Some(servers) = doc
            .get_mut("mcp_servers")
            .and_then(Item::as_inline_table_mut)
    {
        // Servers written inline get the entry spaced like the others.
        servers.fmt();
    }
    Ok(doc.to_string())
}

/// `text`, Codex's config, without Super Voice's entry. `text` as it is when
/// it has none.
///
/// # Errors
///
/// When `text` is not TOML.
pub fn codex_without_super_voice(text: &str) -> Result<String, Error> {
    let mut doc: DocumentMut = text.parse()?;
    let removed = doc
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
        .and_then(|servers| servers.remove(ENTRY))
        .is_some();
    Ok(if removed {
        doc.to_string()
    } else {
        text.to_string()
    })
}

/// Whether `text`, Codex's config, has Super Voice's entry. Text that is not
/// TOML has none.
#[must_use]
pub fn codex_has_super_voice(text: &str) -> bool {
    text.parse::<DocumentMut>().is_ok_and(|doc| {
        doc.get("mcp_servers")
            .and_then(Item::as_table_like)
            .and_then(|servers| servers.get(ENTRY))
            .is_some_and(Item::is_table_like)
    })
}

// OpenCode.
//
// From OpenCode's docs, https://opencode.ai/docs/mcp-servers/ and
// https://opencode.ai/docs/migrate-v1/, and its schema,
// https://opencode.ai/config.json: a remote server is
// `{"type": "remote", "url": .., "headers": {..}}`. OpenCode 2 lists servers
// under `mcp.servers`; OpenCode 1 lists them under `mcp` itself, which
// OpenCode 2 still reads, though its schema calls it deprecated. OpenCode 1
// does not read `mcp.servers`. So the entry goes under `mcp`, which both
// read, unless the file lists its servers under `mcp.servers` already. The
// global file is the first of `opencode.json`, `opencode.jsonc`,
// `.opencode/opencode.json` and `.opencode/opencode.jsonc` in
// `$XDG_CONFIG_HOME/opencode`, the order `opencode mcp add --global` picks
// it in (packages/cli/src/commands/handlers/mcp/add.ts and
// packages/util/src/global-roots.ts in anomalyco/opencode).

/// How long `OpenCode` 1 lets a request to Super Voice run, tool calls
/// included, in milliseconds. Its default, 30 seconds, cuts off a spoken
/// answer. `OpenCode` 2 lets a tool call run 12 hours unless told otherwise.
pub const OPENCODE_TIMEOUT_MS: i32 = 600_000;

/// Keys that make an entry some other kind of server, or keep it off:
/// a local server's, and `enabled` and `disabled`, since connecting turns it
/// on.
const OPENCODE_STALE_KEYS: [&str; 4] = ["command", "environment", "enabled", "disabled"];

/// `OpenCode`'s global config folder: `$XDG_CONFIG_HOME/opencode`, or
/// `~/.config/opencode` when it is not set, on macOS too.
#[must_use]
pub fn opencode_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|config| !config.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;
    Some(config.join("opencode"))
}

/// `OpenCode`'s global config file: the first there is of `opencode.json`,
/// `opencode.jsonc`, `.opencode/opencode.json` and `.opencode/opencode.jsonc`
/// in [`opencode_dir`]. `opencode.json` when there is none.
#[must_use]
pub fn opencode_file() -> Option<PathBuf> {
    let dir = opencode_dir()?;
    let candidates = [
        dir.join("opencode.json"),
        dir.join("opencode.jsonc"),
        dir.join(".opencode/opencode.json"),
        dir.join(".opencode/opencode.jsonc"),
    ];
    let first = candidates[0].clone();
    Some(
        candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .unwrap_or(first),
    )
}

/// `text`, `OpenCode`'s config, with Super Voice's entry: a remote server at
/// `url` that sends `key` as its bearer token. It goes under `mcp`, or under
/// `mcp.servers` when the file lists its servers there. An entry already
/// there is updated in place, keeping the keys and headers it has of its
/// own; one in the other place is taken out, so there is only one.
///
/// A file with comments or trailing commas, which `OpenCode` reads, is
/// edited in place, so they stay.
///
/// # Errors
///
/// When `text` is not JSON, or a key on the way to the entry is not an
/// object.
pub fn opencode_with_super_voice(text: &str, url: &str, key: &str) -> Result<String, Error> {
    let mut root = match json_object(text) {
        Ok(root) => root,
        Err(Error::NotPlainJson(_)) => return jsonc::opencode_with_super_voice(text, url, key),
        Err(e) => return Err(e),
    };
    let mcp = object_at(&mut root, "mcp", "mcp")?;
    let listed_the_new_way = mcp.get("servers").is_some_and(Value::is_object);
    let entry = if listed_the_new_way {
        mcp.shift_remove(ENTRY);
        let servers = object_at(mcp, "servers", "mcp.servers")?;
        object_at(servers, ENTRY, &format!("mcp.servers.{ENTRY}"))?
    } else {
        object_at(mcp, ENTRY, &format!("mcp.{ENTRY}"))?
    };
    for stale in OPENCODE_STALE_KEYS {
        entry.shift_remove(stale);
    }
    entry.insert("type".to_string(), "remote".into());
    entry.insert("url".to_string(), url.into());
    set_bearer(entry, key);
    if !listed_the_new_way {
        // OpenCode 1's timeout, which OpenCode 2 reads too.
        let long_enough = entry
            .get("timeout")
            .and_then(Value::as_i64)
            .is_some_and(|ms| ms >= i64::from(OPENCODE_TIMEOUT_MS));
        if !long_enough {
            entry.insert("timeout".to_string(), OPENCODE_TIMEOUT_MS.into());
        }
    }
    json_text(root, text)
}

/// `text`, `OpenCode`'s config, without Super Voice's entry, under `mcp` or
/// `mcp.servers`. `mcp.servers` and `mcp` go too if that leaves them empty.
/// `text` as it is when it has none.
///
/// A file with comments or trailing commas is edited in place, so they stay.
///
/// # Errors
///
/// When `text` is not JSON.
pub fn opencode_without_super_voice(text: &str) -> Result<String, Error> {
    let mut root = match json_object(text) {
        Ok(root) => root,
        Err(Error::NotPlainJson(_)) => return jsonc::opencode_without_super_voice(text),
        Err(e) => return Err(e),
    };
    let Some(mcp) = root.get_mut("mcp").and_then(Value::as_object_mut) else {
        return Ok(text.to_string());
    };
    let mut removed = mcp.shift_remove(ENTRY).is_some();
    if let Some(servers) = mcp.get_mut("servers").and_then(Value::as_object_mut)
        && servers.shift_remove(ENTRY).is_some()
    {
        removed = true;
        if servers.is_empty() {
            mcp.shift_remove("servers");
        }
    }
    if !removed {
        return Ok(text.to_string());
    }
    if mcp.is_empty() {
        root.shift_remove("mcp");
    }
    json_text(root, text)
}

/// Whether `text`, `OpenCode`'s config, has Super Voice's entry, under `mcp`
/// or `mcp.servers`, comments and trailing commas or not. Text that is not
/// JSON has none.
#[must_use]
pub fn opencode_has_super_voice(text: &str) -> bool {
    let root = match json_object(text) {
        Ok(root) => root,
        Err(Error::NotPlainJson(_)) => return jsonc::opencode_has_super_voice(text),
        Err(_) => return false,
    };
    let mcp = root.get("mcp");
    let v1 = mcp.and_then(|mcp| mcp.get(ENTRY));
    let v2 = mcp
        .and_then(|mcp| mcp.get("servers"))
        .and_then(|servers| servers.get(ENTRY));
    v1.is_some_and(Value::is_object) || v2.is_some_and(Value::is_object)
}

/// `OpenCode`'s config when it has comments or trailing commas, which it
/// reads. Edited in place, so they and everything else in the file stay as
/// they are; the steps are the plain JSON ones above.
mod jsonc {
    use jsonc_parser::ParseOptions;
    use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};

    use super::{ENTRY, Error, OPENCODE_STALE_KEYS, OPENCODE_TIMEOUT_MS};

    fn parse(text: &str) -> Result<CstRootNode, Error> {
        CstRootNode::parse(text, &ParseOptions::default())
            .map_err(|e| Error::NotJson(e.to_string()))
    }

    /// The object at `key` in `object`, made empty when there is none.
    /// `path` names it in the error when `key` holds something else.
    fn object_at(object: &CstObject, key: &str, path: &str) -> Result<CstObject, Error> {
        object
            .object_value_or_create(key)
            .ok_or_else(|| Error::NotATable(path.to_string()))
    }

    /// Give `object` the value `value` at `key`, in place of any it has.
    fn set(object: &CstObject, key: &str, value: CstInputValue) {
        match object.get(key) {
            Some(prop) => prop.set_value(value),
            None => {
                object.append(key, value);
            }
        }
    }

    /// Take `key` out of `object`, saying whether it was there.
    fn remove(object: &CstObject, key: &str) -> bool {
        object
            .get(key)
            .map(jsonc_parser::cst::CstObjectProp::remove)
            .is_some()
    }

    /// `object` has no keys.
    fn is_empty(object: &CstObject) -> bool {
        object.properties().is_empty()
    }

    pub(super) fn opencode_with_super_voice(
        text: &str,
        url: &str,
        key: &str,
    ) -> Result<String, Error> {
        let root = parse(text)?;
        let object = root.object_value_or_create().ok_or(Error::NotAnObject)?;
        let mcp = object_at(&object, "mcp", "mcp")?;
        let listed_the_new_way = mcp.object_value("servers").is_some();
        let entry = if listed_the_new_way {
            remove(&mcp, ENTRY);
            let servers = object_at(&mcp, "servers", "mcp.servers")?;
            object_at(&servers, ENTRY, &format!("mcp.servers.{ENTRY}"))?
        } else {
            object_at(&mcp, ENTRY, &format!("mcp.{ENTRY}"))?
        };
        for stale in OPENCODE_STALE_KEYS {
            remove(&entry, stale);
        }
        set(&entry, "type", "remote".into());
        set(&entry, "url", url.into());
        set_bearer(&entry, key);
        if !listed_the_new_way {
            let long_enough = entry
                .get("timeout")
                .and_then(|prop| prop.value())
                .and_then(|value| value.as_number_lit())
                .and_then(|ms| ms.to_string().parse::<i64>().ok())
                .is_some_and(|ms| ms >= i64::from(OPENCODE_TIMEOUT_MS));
            if !long_enough {
                set(
                    &entry,
                    "timeout",
                    CstInputValue::Number(OPENCODE_TIMEOUT_MS.to_string()),
                );
            }
        }
        Ok(root.to_string())
    }

    /// Give `entry` an `Authorization` header with `key` as its bearer
    /// token, keeping its other headers, as the plain JSON `set_bearer`
    /// does.
    fn set_bearer(entry: &CstObject, key: &str) {
        let headers = entry.object_value_or_set("headers");
        let name = headers
            .properties()
            .iter()
            .filter_map(jsonc_parser::cst::CstObjectProp::decoded_name)
            .find(|name| name.eq_ignore_ascii_case("authorization"))
            .unwrap_or_else(|| "Authorization".to_string());
        set(&headers, &name, format!("Bearer {key}").into());
    }

    pub(super) fn opencode_without_super_voice(text: &str) -> Result<String, Error> {
        let root = parse(text)?;
        let object = root.object_value().ok_or(Error::NotAnObject)?;
        let Some(mcp) = object.object_value("mcp") else {
            return Ok(text.to_string());
        };
        let mut removed = remove(&mcp, ENTRY);
        if let Some(servers) = mcp.object_value("servers")
            && remove(&servers, ENTRY)
        {
            removed = true;
            if is_empty(&servers) {
                remove(&mcp, "servers");
            }
        }
        if !removed {
            return Ok(text.to_string());
        }
        if is_empty(&mcp) {
            remove(&object, "mcp");
        }
        Ok(root.to_string())
    }

    pub(super) fn opencode_has_super_voice(text: &str) -> bool {
        let Some(mcp) = parse(text)
            .ok()
            .and_then(|root| root.object_value())
            .and_then(|object| object.object_value("mcp"))
        else {
            return false;
        };
        mcp.object_value(ENTRY).is_some()
            || mcp
                .object_value("servers")
                .and_then(|servers| servers.object_value(ENTRY))
                .is_some()
    }
}

// Antigravity CLI (`agy`).
//
// From its docs, https://antigravity.google/docs/mcp.md: a server is an entry
// of `mcpServers` in `mcp_config.json`. The user's is
// `~/.gemini/config/mcp_config.json`, which the CLI, the Antigravity App and
// its IDE all read. A remote server is `serverUrl` (Streamable HTTP; `httpUrl`
// is not taken), with `headers` as they are, and `timeoutSeconds` for both
// connecting and each tool call, 180 by default.

/// Keys that make an entry some other kind of server, a stdio one, or name
/// its endpoint another way.
const ANTIGRAVITY_STALE_KEYS: [&str; 7] =
    ["url", "httpUrl", "type", "command", "args", "cwd", "env"];

/// How long a tool call may run: `listen` waits for the user, often longer
/// than the default 3 minutes.
const ANTIGRAVITY_TIMEOUT_SECONDS: u64 = 600;

/// The Antigravity CLI's MCP config: `.gemini/config/mcp_config.json` in the
/// home directory.
#[must_use]
pub fn antigravity_file() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".gemini/config/mcp_config.json"))
}

/// `text`, the Antigravity CLI's MCP config, with Super Voice's entry: the
/// server at `url` over Streamable HTTP, sending `key` as its bearer token,
/// with time for `listen`. An entry already there is updated in place,
/// keeping the keys and headers it has of its own.
///
/// # Errors
///
/// When `text` is not plain JSON, or a key on the way to the entry is not an
/// object.
pub fn antigravity_with_super_voice(text: &str, url: &str, key: &str) -> Result<String, Error> {
    let mut root = json_object(text)?;
    let servers = object_at(&mut root, "mcpServers", "mcpServers")?;
    let entry = object_at(servers, ENTRY, &format!("mcpServers.{ENTRY}"))?;
    for stale in ANTIGRAVITY_STALE_KEYS {
        entry.shift_remove(stale);
    }
    entry.insert("serverUrl".to_string(), url.into());
    set_bearer(entry, key);
    entry.insert(
        "timeoutSeconds".to_string(),
        ANTIGRAVITY_TIMEOUT_SECONDS.into(),
    );
    // The tools Super Voice's plugin calls itself, never the model.
    entry.insert(
        "disabledTools".to_string(),
        super::PLUGIN_TOOLS.into_iter().collect(),
    );
    json_text(root, text)
}

/// `text`, the Antigravity CLI's MCP config, without Super Voice's entry.
/// `mcpServers` goes too if that leaves it empty. `text` as it is when it
/// has none.
///
/// # Errors
///
/// When `text` is not plain JSON.
pub fn antigravity_without_super_voice(text: &str) -> Result<String, Error> {
    let mut root = json_object(text)?;
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Ok(text.to_string());
    };
    if servers.shift_remove(ENTRY).is_none() {
        return Ok(text.to_string());
    }
    if servers.is_empty() {
        root.shift_remove("mcpServers");
    }
    json_text(root, text)
}

/// Whether `text`, the Antigravity CLI's MCP config, has Super Voice's entry.
/// Text that is not plain JSON has none.
#[must_use]
pub fn antigravity_has_super_voice(text: &str) -> bool {
    json_object(text).is_ok_and(|root| {
        root.get("mcpServers")
            .and_then(|servers| servers.get(ENTRY))
            .is_some_and(Value::is_object)
    })
}

// The JSON files.

/// `text` read as a JSON object. Empty text is an empty object.
fn json_object(text: &str) -> Result<Map<String, Value>, Error> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(text)? {
        Value::Object(root) => Ok(root),
        _ => Err(Error::NotAnObject),
    }
}

/// The object at `key` in `object`, made empty when there is none. `path`
/// names it in the error when `key` holds something else.
fn object_at<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a mut Map<String, Value>, Error> {
    object
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::NotATable(path.to_string()))
}

/// Give `entry` an `Authorization` header with `key` as its bearer token,
/// keeping its other headers. A header named in other letters, like
/// `authorization`, is the same header, so it is the one updated.
fn set_bearer(entry: &mut Map<String, Value>, key: &str) {
    let bearer = Value::String(format!("Bearer {key}"));
    if !entry.get("headers").is_some_and(Value::is_object) {
        entry.insert("headers".to_string(), Value::Object(Map::new()));
    }
    let Some(headers) = entry.get_mut("headers").and_then(Value::as_object_mut) else {
        return;
    };
    let name = headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case("authorization"))
        .cloned()
        .unwrap_or_else(|| "Authorization".to_string());
    headers.insert(name, bearer);
}

/// `root` as the file's text: indented as `original` is, two spaces when it
/// shows none, and ending in a newline.
fn json_text(root: Map<String, Value>, original: &str) -> Result<String, Error> {
    let indent = json_indent(original);
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    Value::Object(root)
        .serialize(&mut serializer)
        .map_err(std::io::Error::other)?;
    let mut text = String::from_utf8(out).map_err(std::io::Error::other)?;
    text.push('\n');
    Ok(text)
}

/// The indent of the first indented line of `text`.
fn json_indent(text: &str) -> &str {
    text.lines()
        .map(|line| {
            let body = line.trim_start_matches([' ', '\t']);
            &line[..line.len() - body.len()]
        })
        .find(|indent| !indent.is_empty())
        .unwrap_or("  ")
}

// The files.

/// The text of the config file at `path`. A file that is not there reads as
/// empty.
///
/// # Errors
///
/// When the file is there and cannot be read.
pub fn read(path: &Path) -> Result<String, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// Rewrite the config file at `path` with `transform`, such as
/// [`antigravity_without_super_voice`], and say whether it changed. A file that is
/// not there is `transform`ed from empty text.
///
/// The new text is written to a file beside it, then put in its place, so the
/// assistant never reads half a file. The file keeps its permissions. A new
/// one is readable by its owner alone, since it may hold a key, and the
/// directories it is in are made. A symlink is written through: the link
/// stays, and the file it points at changes.
///
/// # Errors
///
/// When `transform` fails, or the file cannot be read or written.
pub fn update(
    path: &Path,
    transform: impl FnOnce(&str) -> Result<String, Error>,
) -> Result<bool, Error> {
    let text = read(path)?;
    let updated = transform(&text)?;
    if updated == text {
        return Ok(false);
    }
    write_atomically(path, &updated, false)?;
    Ok(true)
}

/// [`update`], for a file `transform` writes a key into, such as
/// [`antigravity_with_super_voice`]: afterwards only its owner can read it,
/// even when others could before. A file left as it was is narrowed too,
/// since it may hold the key from an earlier write.
///
/// # Errors
///
/// When `transform` fails, or the file cannot be read, written or narrowed.
pub fn update_holding_key(
    path: &Path,
    transform: impl FnOnce(&str) -> Result<String, Error>,
) -> Result<bool, Error> {
    let text = read(path)?;
    let updated = transform(&text)?;
    if updated != text {
        write_atomically(path, &updated, true)?;
        return Ok(true);
    }
    #[cfg(unix)]
    if let Ok(metadata) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode() & 0o7777;
        if mode != owner_only(mode) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(owner_only(mode)))?;
        }
    }
    Ok(false)
}

/// `mode` with the group's and others' permissions taken away.
#[cfg(unix)]
const fn owner_only(mode: u32) -> u32 {
    mode & 0o700
}

/// Write `text` to `path` through a temporary file in the same directory,
/// readable by its owner alone when `private`.
fn write_atomically(path: &Path, text: &str, private: bool) -> std::io::Result<()> {
    /// Tells apart this process's temporary files.
    static NEXT: AtomicU32 = AtomicU32::new(0);

    let path = match std::fs::canonicalize(path) {
        Ok(target) => target,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(e) => return Err(e),
    };
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("the path names no file"))?;
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;

    let temp = dir.join(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = write_then_rename(&temp, &path, text, private);
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// Write `text` to `temp`, a new file, with `path`'s permissions, or the
/// owner's alone when there is no `path` yet or it is `private`; then put it
/// in `path`'s place.
fn write_then_rename(temp: &Path, path: &Path, text: &str, private: bool) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        let mode = std::fs::metadata(path).map_or(0o600, |m| m.permissions().mode() & 0o7777);
        let mode = if private { owner_only(mode) } else { mode };
        options.mode(mode);
        mode
    };
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(temp)?;
    // The mode it was made with is narrowed by the umask. It gets it exactly.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "http://127.0.0.1:7300/v1/mcp";
    const HELPER: &str = "/usr/local/bin/voice assistant-header codex";

    // Codex.

    #[test]
    fn codex_entry_in_an_empty_file() {
        let text = codex_with_super_voice("", URL, HELPER).unwrap();
        assert_eq!(
            text,
            "[mcp_servers.super-voice]\n\
             url = \"http://127.0.0.1:7300/v1/mcp\"\n\
             http_headers_helper = \"/usr/local/bin/voice assistant-header codex\"\n\
             tool_timeout_sec = 600\n"
        );
        assert!(codex_has_super_voice(&text));
        assert!(!codex_has_super_voice(""));
    }

    /// A config with settings, comments, other servers and a table after
    /// them. Super Voice's entry joins the other servers, and taking it out
    /// gives back the file as it was.
    const CODEX_CONFIG: &str = "\
# My settings
model = \"gpt-5\" # the default

[mcp_servers.docs]
command = \"docs-mcp\"  # local

# Fast work.
[profiles.fast]
model = \"o4-mini\"
";

    #[test]
    fn codex_entry_keeps_the_rest_of_the_file() {
        let text = codex_with_super_voice(CODEX_CONFIG, URL, HELPER).unwrap();
        assert_eq!(
            text,
            "\
# My settings
model = \"gpt-5\" # the default

[mcp_servers.docs]
command = \"docs-mcp\"  # local

[mcp_servers.super-voice]
url = \"http://127.0.0.1:7300/v1/mcp\"
http_headers_helper = \"/usr/local/bin/voice assistant-header codex\"
tool_timeout_sec = 600

# Fast work.
[profiles.fast]
model = \"o4-mini\"
"
        );
        assert_eq!(codex_without_super_voice(&text).unwrap(), CODEX_CONFIG);
    }

    /// A file of settings alone gets the entry after them.
    #[test]
    fn codex_entry_after_settings() {
        let text = codex_with_super_voice("model = \"gpt-5\"\n", URL, HELPER).unwrap();
        assert!(
            text.starts_with("model = \"gpt-5\"\n\n[mcp_servers.super-voice]\n"),
            "{text}"
        );
        assert_eq!(
            codex_without_super_voice(&text).unwrap(),
            "model = \"gpt-5\"\n"
        );
    }

    /// Connecting again updates the entry: one entry, the new values, the
    /// user's own keys kept, and the ones that would stop it working gone.
    #[test]
    fn codex_entry_is_updated_not_added_again() {
        let old = "\
[mcp_servers.super-voice]
url = \"http://127.0.0.1:7000/v1/mcp\"
bearer_token_env_var = \"OLD_TOKEN\"
enabled = false
tool_timeout_sec = 30
default_tools_approval_mode = \"approve\"
";
        let text = codex_with_super_voice(old, URL, HELPER).unwrap();
        assert_eq!(
            text,
            "\
[mcp_servers.super-voice]
url = \"http://127.0.0.1:7300/v1/mcp\"
tool_timeout_sec = 600
default_tools_approval_mode = \"approve\"
http_headers_helper = \"/usr/local/bin/voice assistant-header codex\"
"
        );
        assert_eq!(codex_with_super_voice(&text, URL, HELPER).unwrap(), text);

        // A longer timeout the user set is kept.
        let longer = codex_with_super_voice(
            "[mcp_servers.super-voice]\ntool_timeout_sec = 900.5\n",
            URL,
            HELPER,
        )
        .unwrap();
        assert!(longer.contains("tool_timeout_sec = 900.5\n"), "{longer}");
        assert_eq!(longer.matches("tool_timeout_sec").count(), 1, "{longer}");
    }

    /// Servers written as an inline table stay one.
    #[test]
    fn codex_entry_in_an_inline_table() {
        let inline = "mcp_servers = { docs = { command = \"docs-mcp\" } }\n";
        let text = codex_with_super_voice(inline, URL, HELPER).unwrap();
        assert!(
            text.starts_with("mcp_servers = { docs = { command = \"docs-mcp\" }, super-voice = {"),
            "{text}"
        );
        assert!(codex_has_super_voice(&text));
        let without = codex_without_super_voice(&text).unwrap();
        assert!(!codex_has_super_voice(&without));
        assert!(
            without.contains("docs = { command = \"docs-mcp\" }"),
            "{without}"
        );
    }

    #[test]
    fn codex_removing_an_absent_entry_changes_nothing() {
        for text in [
            "",
            CODEX_CONFIG,
            "[mcp_servers]\n# none yet\n",
            "mcp_servers = 3\n",
        ] {
            assert_eq!(codex_without_super_voice(text).unwrap(), text);
            assert!(!codex_has_super_voice(text), "{text}");
        }
    }

    #[test]
    fn codex_refuses_what_it_cannot_read() {
        assert!(matches!(
            codex_with_super_voice("model = ", URL, HELPER),
            Err(Error::Toml(_))
        ));
        assert!(matches!(
            codex_without_super_voice("[x"),
            Err(Error::Toml(_))
        ));
        assert!(!codex_has_super_voice("[x"));
        assert!(matches!(
            codex_with_super_voice("mcp_servers = 3\n", URL, HELPER),
            Err(Error::NotATable(path)) if path == "mcp_servers"
        ));
        assert!(matches!(
            codex_with_super_voice("[mcp_servers]\nsuper-voice = \"x\"\n", URL, HELPER),
            Err(Error::NotATable(path)) if path == "mcp_servers.super-voice"
        ));
    }

    /// The helper runs through `sh -c`, so a path with a space or a quote in
    /// it is quoted.
    #[test]
    fn codex_helper_quotes_the_program() {
        assert_eq!(codex_headers_helper("/usr/local/bin/voice"), HELPER);
        assert_eq!(
            codex_headers_helper("voice"),
            "voice assistant-header codex"
        );
        assert_eq!(
            codex_headers_helper("/Applications/Super Voice.app/Contents/MacOS/voice"),
            "'/Applications/Super Voice.app/Contents/MacOS/voice' assistant-header codex"
        );
        assert_eq!(
            codex_headers_helper("/home/o'neil/voice"),
            r"'/home/o'\''neil/voice' assistant-header codex"
        );
    }

    // OpenCode.

    /// An empty file gets the entry under `mcp`, which `OpenCode` 1 and 2
    /// both read.
    #[test]
    fn opencode_entry_in_an_empty_file() {
        let text = opencode_with_super_voice("", URL, "k1").unwrap();
        assert_eq!(
            text,
            r#"{
  "mcp": {
    "super-voice": {
      "type": "remote",
      "url": "http://127.0.0.1:7300/v1/mcp",
      "headers": {
        "Authorization": "Bearer k1"
      },
      "timeout": 600000
    }
  }
}
"#
        );
        assert!(opencode_has_super_voice(&text));
        assert!(!opencode_has_super_voice(""));
        assert_eq!(opencode_without_super_voice(&text).unwrap(), "{}\n");
    }

    /// A config with other keys and servers, indented four spaces. The keys
    /// keep their order and indent, and taking the entry out gives back the
    /// file as it was.
    const OPENCODE_CONFIG: &str = r#"{
    "$schema": "https://opencode.ai/config.json",
    "theme": "opencode",
    "mcp": {
        "docs": {
            "type": "local",
            "command": [
                "docs-mcp"
            ]
        }
    },
    "model": "anthropic/claude-sonnet"
}
"#;

    #[test]
    fn opencode_entry_keeps_the_rest_of_the_file() {
        let text = opencode_with_super_voice(OPENCODE_CONFIG, URL, "k1").unwrap();
        let keys: Vec<String> = json_object(&text).unwrap().keys().cloned().collect();
        assert_eq!(keys, ["$schema", "theme", "mcp", "model"]);
        assert!(
            text.contains(
                "        },\n        \"super-voice\": {\n            \"type\": \"remote\",\n"
            ),
            "{text}"
        );
        assert_eq!(
            opencode_without_super_voice(&text).unwrap(),
            OPENCODE_CONFIG
        );
    }

    /// A file that lists its servers under `mcp.servers` is `OpenCode` 2's
    /// own, so the entry goes there, with no timeout of `OpenCode` 1's. One
    /// left under `mcp` beside it is taken out.
    #[test]
    fn opencode_entry_joins_servers_listed_the_new_way() {
        let config = r#"{
  "mcp": {
    "super-voice": {
      "type": "remote",
      "url": "http://127.0.0.1:7000/v1/mcp"
    },
    "servers": {
      "docs": {
        "type": "local",
        "command": [
          "docs-mcp"
        ]
      }
    }
  }
}
"#;
        let text = opencode_with_super_voice(config, URL, "k1").unwrap();
        assert_eq!(
            text,
            r#"{
  "mcp": {
    "servers": {
      "docs": {
        "type": "local",
        "command": [
          "docs-mcp"
        ]
      },
      "super-voice": {
        "type": "remote",
        "url": "http://127.0.0.1:7300/v1/mcp",
        "headers": {
          "Authorization": "Bearer k1"
        }
      }
    }
  }
}
"#
        );
        assert!(opencode_has_super_voice(&text));
        let without = opencode_without_super_voice(&text).unwrap();
        assert!(!opencode_has_super_voice(&without));
        assert!(without.contains("\"docs\""), "{without}");

        // Alone under `mcp.servers`, it takes them both with it.
        let alone = r#"{"mcp": {"servers": {}}}"#;
        let text = opencode_with_super_voice(alone, URL, "k1").unwrap();
        assert!(text.contains("\"servers\""), "{text}");
        assert_eq!(opencode_without_super_voice(&text).unwrap(), "{}\n");
    }

    /// Connecting again updates the entry: the new key in the same header,
    /// the user's own headers and keys kept, and the entry turned on.
    #[test]
    fn opencode_entry_is_updated_not_added_again() {
        let old = r#"{
  "mcp": {
    "super-voice": {
      "type": "remote",
      "url": "http://127.0.0.1:7000/v1/mcp",
      "enabled": false,
      "headers": {
        "authorization": "Bearer old",
        "X-Trace": "1"
      },
      "timeout": 900000
    }
  }
}
"#;
        let text = opencode_with_super_voice(old, URL, "k2").unwrap();
        assert_eq!(
            text,
            r#"{
  "mcp": {
    "super-voice": {
      "type": "remote",
      "url": "http://127.0.0.1:7300/v1/mcp",
      "headers": {
        "authorization": "Bearer k2",
        "X-Trace": "1"
      },
      "timeout": 900000
    }
  }
}
"#
        );
        assert_eq!(opencode_with_super_voice(&text, URL, "k2").unwrap(), text);
    }

    #[test]
    fn opencode_removing_an_absent_entry_changes_nothing() {
        for text in [
            "",
            OPENCODE_CONFIG,
            r#"{"mcp": {"servers": {"docs": {}}}}"#,
            r#"{"mcp": 3}"#,
        ] {
            assert_eq!(opencode_without_super_voice(text).unwrap(), text);
            assert!(!opencode_has_super_voice(text), "{text}");
        }
    }

    /// An `opencode.jsonc` like most people's: a comment, and trailing
    /// commas, which `OpenCode` reads.
    const OPENCODE_JSONC: &str = r#"{
  "$schema": "https://opencode.ai/config.json",
  // Local models.
  "provider": {
    "ollama": {
      "options": {
        "baseURL": "http://127.0.0.1:11434/v1",
      },
    },
  },
}
"#;

    /// Such a file is edited in place: its comment and trailing commas stay,
    /// and taking the entry out gives back the file as it was.
    #[test]
    fn opencode_keeps_comments_and_trailing_commas() {
        let text = opencode_with_super_voice(OPENCODE_JSONC, URL, "k1").unwrap();
        assert!(
            text.starts_with(
                &OPENCODE_JSONC[..OPENCODE_JSONC
                    .find(
                        "  },
}"
                    )
                    .unwrap()]
            ),
            "{text}"
        );
        assert!(text.contains("// Local models."), "{text}");
        assert!(
            text.contains("\"baseURL\": \"http://127.0.0.1:11434/v1\",\n"),
            "{text}"
        );
        assert!(opencode_has_super_voice(&text), "{text}");
        let root =
            jsonc_parser::cst::CstRootNode::parse(&text, &jsonc_parser::ParseOptions::default())
                .unwrap();
        let entry = root
            .object_value()
            .and_then(|root| root.object_value("mcp"))
            .and_then(|mcp| mcp.object_value(ENTRY))
            .unwrap();
        let raw = |object: &jsonc_parser::cst::CstObject, key: &str| {
            object
                .get(key)
                .and_then(|prop| prop.value())
                .map(|value| value.to_string())
        };
        assert_eq!(raw(&entry, "type").as_deref(), Some("\"remote\""));
        assert_eq!(raw(&entry, "url"), Some(format!("\"{URL}\"")));
        assert_eq!(
            entry
                .object_value("headers")
                .and_then(|headers| raw(&headers, "Authorization"))
                .as_deref(),
            Some("\"Bearer k1\"")
        );
        assert_eq!(
            raw(&entry, "timeout"),
            Some(OPENCODE_TIMEOUT_MS.to_string())
        );

        // Connecting again only changes the key.
        let again = opencode_with_super_voice(&text, URL, "k2").unwrap();
        assert_eq!(again, text.replace("Bearer k1", "Bearer k2"));

        assert_eq!(opencode_without_super_voice(&text).unwrap(), OPENCODE_JSONC);
        assert!(!opencode_has_super_voice(OPENCODE_JSONC));
        assert_eq!(
            opencode_without_super_voice(OPENCODE_JSONC).unwrap(),
            OPENCODE_JSONC
        );
    }

    /// With comments, an entry under `mcp.servers` is found and updated
    /// there, as it is in plain JSON.
    #[test]
    fn opencode_jsonc_keeps_the_new_way_of_listing() {
        let text = "{\n  // Servers.\n  \"mcp\": {\"servers\": {\"docs\": {}}},\n}\n";
        let connected = opencode_with_super_voice(text, URL, "k1").unwrap();
        assert!(opencode_has_super_voice(&connected), "{connected}");
        assert!(connected.contains("\"servers\""), "{connected}");
        assert!(!connected.contains("timeout"), "{connected}");
        assert_eq!(
            opencode_without_super_voice(&connected)
                .unwrap()
                .contains(ENTRY),
            false
        );
    }

    /// What can't be read, or holds something else on the way to the entry,
    /// is refused and left as it is.
    #[test]
    fn opencode_refuses_what_it_cannot_edit() {
        assert!(matches!(
            opencode_with_super_voice("{ nope", URL, "k1"),
            Err(Error::NotJson(_))
        ));
        assert!(matches!(
            opencode_with_super_voice("[]", URL, "k1"),
            Err(Error::NotAnObject)
        ));
        for text in [r#"{"mcp": 3}"#, "{\n  // Off.\n  \"mcp\": 3,\n}\n"] {
            assert!(matches!(
                opencode_with_super_voice(text, URL, "k1"),
                Err(Error::NotATable(path)) if path == "mcp"
            ));
        }
        assert!(!opencode_has_super_voice("{ nope"));
    }

    // Antigravity CLI.

    #[test]
    fn antigravity_entry_in_an_empty_file() {
        let text = antigravity_with_super_voice("", URL, "k1").unwrap();
        assert_eq!(
            text,
            r#"{
  "mcpServers": {
    "super-voice": {
      "serverUrl": "http://127.0.0.1:7300/v1/mcp",
      "headers": {
        "Authorization": "Bearer k1"
      },
      "timeoutSeconds": 600,
      "disabledTools": [
        "stop",
        "status",
        "announce",
        "session",
        "turn"
      ]
    }
  }
}
"#
        );
        assert!(antigravity_has_super_voice(&text));
        assert!(!antigravity_has_super_voice(""));
        assert_eq!(antigravity_without_super_voice(&text).unwrap(), "{}\n");
    }

    const ANTIGRAVITY_CONFIG: &str = "{\n\t\"theme\": \"GitHub\",\n\t\"mcpServers\": {\n\t\t\"docs\": {\n\t\t\t\"command\": \"docs-mcp\"\n\t\t}\n\t},\n\t\"selectedAuthType\": \"oauth-personal\"\n}\n";

    /// A config indented with tabs keeps them, and its order.
    #[test]
    fn antigravity_entry_keeps_the_rest_of_the_file() {
        let text = antigravity_with_super_voice(ANTIGRAVITY_CONFIG, URL, "k1").unwrap();
        assert_eq!(
            text,
            "{\n\t\"theme\": \"GitHub\",\n\t\"mcpServers\": {\n\t\t\"docs\": {\n\t\t\t\"command\": \"docs-mcp\"\n\t\t},\n\t\t\"super-voice\": {\n\t\t\t\"serverUrl\": \"http://127.0.0.1:7300/v1/mcp\",\n\t\t\t\"headers\": {\n\t\t\t\t\"Authorization\": \"Bearer k1\"\n\t\t\t},\n\t\t\t\"timeoutSeconds\": 600,\n\t\t\t\"disabledTools\": [\n\t\t\t\t\"stop\",\n\t\t\t\t\"status\",\n\t\t\t\t\"announce\",\n\t\t\t\t\"session\",\n\t\t\t\t\"turn\"\n\t\t\t]\n\t\t}\n\t},\n\t\"selectedAuthType\": \"oauth-personal\"\n}\n"
        );
        assert_eq!(
            antigravity_without_super_voice(&text).unwrap(),
            ANTIGRAVITY_CONFIG
        );
    }

    /// Connecting again updates the entry, keeps what the user set, such as
    /// `disabled`, and drops another way of naming the endpoint.
    #[test]
    fn antigravity_entry_is_updated_not_added_again() {
        let old = r#"{
  "mcpServers": {
    "super-voice": {
      "url": "http://127.0.0.1:7000/mcp",
      "disabled": false,
      "headers": {
        "Authorization": "Bearer old"
      }
    }
  }
}
"#;
        let text = antigravity_with_super_voice(old, URL, "k2").unwrap();
        assert_eq!(
            text,
            r#"{
  "mcpServers": {
    "super-voice": {
      "disabled": false,
      "headers": {
        "Authorization": "Bearer k2"
      },
      "serverUrl": "http://127.0.0.1:7300/v1/mcp",
      "timeoutSeconds": 600,
      "disabledTools": [
        "stop",
        "status",
        "announce",
        "session",
        "turn"
      ]
    }
  }
}
"#
        );
        assert_eq!(
            antigravity_with_super_voice(&text, URL, "k2").unwrap(),
            text
        );
    }

    #[test]
    fn antigravity_removing_an_absent_entry_changes_nothing() {
        for text in ["", ANTIGRAVITY_CONFIG, "{}", r#"{"mcpServers": []}"#] {
            assert_eq!(antigravity_without_super_voice(text).unwrap(), text);
            assert!(!antigravity_has_super_voice(text), "{text}");
        }
    }

    #[test]
    fn antigravity_refuses_what_it_would_lose() {
        let commented = "{\n  /* Mine. */\n  \"theme\": \"GitHub\"\n}\n";
        assert!(matches!(
            antigravity_with_super_voice(commented, URL, "k1"),
            Err(Error::NotPlainJson(_))
        ));
        assert!(matches!(
            antigravity_without_super_voice(commented),
            Err(Error::NotPlainJson(_))
        ));
        assert!(!antigravity_has_super_voice(commented));
        assert!(matches!(
            antigravity_with_super_voice(r#"{"mcpServers": []}"#, URL, "k1"),
            Err(Error::NotATable(path)) if path == "mcpServers"
        ));
    }

    // The files.

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    /// A new file is made with its directories, readable by its owner alone.
    #[test]
    fn update_makes_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gemini/config/mcp_config.json");
        assert_eq!(read(&path).unwrap(), "");

        let changed = update(&path, |text| antigravity_with_super_voice(text, URL, "k1")).unwrap();
        assert!(changed);
        assert!(antigravity_has_super_voice(&read(&path).unwrap()));
        #[cfg(unix)]
        assert_eq!(mode(&path), 0o600);
        // No temporary file is left beside it.
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    /// A rewritten file keeps its permissions.
    #[cfg(unix)]
    #[test]
    fn update_keeps_the_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, CODEX_CONFIG).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(update(&path, |text| codex_with_super_voice(text, URL, HELPER)).unwrap());
        assert_eq!(mode(&path), 0o644);
        assert!(update(&path, codex_without_super_voice).unwrap());
        assert_eq!(read(&path).unwrap(), CODEX_CONFIG);
        assert_eq!(mode(&path), 0o644);
    }

    /// A file a key is written into is readable by its owner alone after,
    /// even one others could read before, and even when it already had the
    /// entry. Taking the entry out leaves it so.
    #[cfg(unix)]
    #[test]
    fn a_file_holding_a_key_is_the_owners_alone() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_config.json");
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let connect = |text: &str| antigravity_with_super_voice(text, URL, "k1");
        assert!(update_holding_key(&path, connect).unwrap());
        assert_eq!(mode(&path), 0o600);

        // Written earlier with the old permissions, and unchanged now.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
        assert!(!update_holding_key(&path, connect).unwrap());
        assert_eq!(mode(&path), 0o600);

        assert!(update(&path, antigravity_without_super_voice).unwrap());
        assert_eq!(mode(&path), 0o600);
    }

    /// A change that changes nothing writes nothing, and makes no file.
    #[test]
    fn update_writes_nothing_for_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing/opencode.json");
        assert!(!update(&path, opencode_without_super_voice).unwrap());
        assert!(!path.exists());
        assert!(!dir.path().join("missing").exists());
    }

    /// A transform that fails leaves the file as it was: here a comment the
    /// Antigravity CLI's plain JSON would lose.
    #[test]
    fn update_leaves_a_file_it_would_lose() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_config.json");
        let commented = "{\n  // Mine.\n  \"mcpServers\": {}\n}\n";
        std::fs::write(&path, commented).unwrap();
        assert!(matches!(
            update(&path, |text| antigravity_with_super_voice(text, URL, "k1")),
            Err(Error::NotPlainJson(_))
        ));
        assert_eq!(read(&path).unwrap(), commented);
    }

    /// A symlinked file, as dotfile managers make, is written through.
    #[cfg(unix)]
    #[test]
    fn update_writes_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dotfiles/settings.json");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, ANTIGRAVITY_CONFIG).unwrap();
        let link = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(update(&link, |text| antigravity_with_super_voice(text, URL, "k1")).unwrap());
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(antigravity_has_super_voice(&read(&target).unwrap()));
    }
}
