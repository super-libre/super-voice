// SPDX-License-Identifier: GPL-3.0-only
//! What the daemon and its clients say about AI assistants: which ones Super
//! Voice can connect to, where each one stands, and the keys made by hand for
//! other MCP clients.
//!
//! The daemon serves Super Voice's MCP server at [`AssistantList::address`].
//! Connecting an assistant makes a key for it alone and writes Super Voice
//! into that assistant's own config. Disconnecting takes the entry out and
//! revokes the key. The daemon does both, so a client shows where each
//! assistant stands and never writes an assistant's config itself.

use serde::{Deserialize, Serialize};
use super_engine_protocol::wire_enum_strings;

/// An AI assistant Super Voice can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssistantId {
    /// Claude Code. Super Voice reaches it through its plugin.
    ClaudeCode,
    /// The Codex CLI.
    Codex,
    /// `OpenCode`.
    OpenCode,
    /// The Antigravity CLI, `agy`: Google's terminal agent, which took the
    /// Gemini CLI's place.
    Antigravity,
}

wire_enum_strings!(AssistantId {
    ClaudeCode => "claude-code",
    Codex => "codex",
    OpenCode => "opencode",
    Antigravity => "antigravity",
});

impl AssistantId {
    /// Every assistant, in the order a list shows them.
    pub const ALL: [Self; 4] = [
        Self::ClaudeCode,
        Self::Codex,
        Self::OpenCode,
        Self::Antigravity,
    ];

    /// The assistant's name, as people know it.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity CLI",
        }
    }
}

/// Where one assistant stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantState {
    /// The daemon found no sign of it.
    NotFound,
    /// It is here, and Super Voice is not in its config.
    Found,
    /// Super Voice is in its config, with a key of its own.
    Connected,
}

wire_enum_strings!(AssistantState {
    NotFound => "not_found",
    Found => "found",
    Connected => "connected",
});

/// One assistant in an [`AssistantList`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Assistant {
    pub id: AssistantId,
    pub state: AssistantState,
    /// Whether Super Voice says aloud when the assistant needs the user, like
    /// Claude Code's own notices that it needs a permission or an answer.
    /// Claude Code, Codex and `OpenCode` announce, through their plugins.
    pub announce_when_needed: bool,
    /// Whether its `listen` calls are heard with the context they bring from
    /// the conversation, on top of the user's own.
    pub conversation_context: bool,
    /// Whether its plugin shows Talk above the prompt: Claude Code's and
    /// `OpenCode`'s, the assistants with a Talk of their own.
    pub show_talk: bool,
    /// Its open sessions, oldest first.
    #[serde(default)]
    pub sessions: Vec<AssistantSession>,
}

/// An open session of an assistant: one conversation, like one Claude Code
/// session or one Codex thread. Listed from its first request, or from when
/// its plugin says it started, until it ends or goes quiet.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AssistantSession {
    /// The session's id, as the assistant names it. Absent when it names
    /// none: then all its requests are one session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The folder it works in, when the assistant says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(example = "/home/me/super-voice"))]
    pub folder: Option<String>,
    /// How many agents besides the session's own, like subagents, have made
    /// requests.
    pub agents: usize,
    /// Speech it started is playing.
    pub speaking: bool,
    /// It is recording the user.
    pub listening: bool,
    /// When it was first seen, in RFC 3339.
    #[cfg_attr(feature = "openapi", schema(example = "2026-11-20T09:15:00Z"))]
    pub started_at: String,
    /// When it last spoke or listened, in RFC 3339. Absent until it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(example = "2026-11-20T09:42:10Z"))]
    pub last_used_at: Option<String>,
}

/// A key made by hand for another MCP client. The key itself is shown only
/// once, in the [`NewKey`] that made it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CustomKey {
    /// What names the key, to revoke it.
    pub id: String,
    /// The name the user gave the key.
    #[cfg_attr(feature = "openapi", schema(example = "My editor"))]
    pub name: String,
    /// When the key was made, in RFC 3339.
    #[cfg_attr(feature = "openapi", schema(example = "2026-11-19T20:15:00Z"))]
    pub created_at: String,
    /// The open sessions of the client it was made for, oldest first.
    #[serde(default)]
    pub sessions: Vec<AssistantSession>,
}

/// Every assistant, every key made by hand, and where the MCP server is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AssistantList {
    /// Every assistant, in [`AssistantId::ALL`]'s order.
    pub assistants: Vec<Assistant>,
    /// The keys made by hand, in the order they were made.
    pub custom_keys: Vec<CustomKey>,
    /// Where an MCP client reaches Super Voice.
    #[cfg_attr(feature = "openapi", schema(example = "http://127.0.0.1:7300/v1/mcp"))]
    pub address: String,
}

/// A key just made by hand, with the key itself. This is the only time the
/// key is shown.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NewKey {
    /// What names the key, to revoke it.
    pub id: String,
    /// The name the user gave the key.
    #[cfg_attr(feature = "openapi", schema(example = "My editor"))]
    pub name: String,
    /// The key. An MCP client sends it as `Authorization: Bearer <key>`.
    pub key: String,
    /// Where an MCP client reaches Super Voice.
    #[cfg_attr(feature = "openapi", schema(example = "http://127.0.0.1:7300/v1/mcp"))]
    pub address: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list shows the assistants in the same order the wire names them.
    #[test]
    fn all_lists_every_assistant_once() {
        let wire: Vec<&str> = AssistantId::ALL.iter().map(|id| id.as_wire_str()).collect();
        assert_eq!(wire, AssistantId::WIRE_VARIANTS);
    }

    #[test]
    fn an_assistant_crosses_the_wire_by_its_token() {
        let assistant = Assistant {
            id: AssistantId::ClaudeCode,
            state: AssistantState::NotFound,
            announce_when_needed: true,
            conversation_context: false,
            show_talk: true,
            sessions: Vec::new(),
        };
        let json = serde_json::to_value(&assistant).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "id": "claude-code",
                "state": "not_found",
                "announce_when_needed": true,
                "conversation_context": false,
                "show_talk": true,
                "sessions": [],
            })
        );
        let back: Assistant = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, assistant);
    }
}
