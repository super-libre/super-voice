// SPDX-License-Identifier: GPL-3.0-only
//! The AI assistants Super Voice connects to, which the daemon owns:
//! `/v1/assistants`.
//!
//! The daemon owns them as it owns the keyboard shortcuts, so every client
//! sees the same answer, and so a client that cannot write another program's
//! files, like a sandboxed app, can still connect one. Connecting an
//! assistant makes it a key of its own, through the auth layer, and writes
//! Super Voice into the assistant's own config:
//!
//! 1. Claude Code: the daemon runs `claude plugin install`, and the plugin
//!    gets its key from `voice assistant-header claude-code`.
//! 2. Codex, `OpenCode` and the Antigravity CLI: an entry in their config files
//!    ([`super_voice_shared::assistant_configs`]).
//!
//! Disconnecting takes the entry out and revokes the key, which also cancels
//! the key's calls in flight ([`calls`]). A key made by hand, for any other
//! MCP client, is named by the user and only revoked.
//!
//! The work is `daemon::assistant_handlers`. Contract:
//! `docs/protocol/endpoints/v1/assistants.md`.

pub mod calls;
pub mod sessions;

use log::{info, warn};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use super_voice_shared::SUPER_VOICE;
use super_voice_shared::assistant_configs;
use super_voice_shared::models::assistants::{AssistantId, AssistantState};

/// Where an MCP client reaches Super Voice on the daemon's TCP listener.
pub const MCP_PATH: &str = "/v1/mcp";

/// What a key made for an assistant may do: speak, and hear the user.
pub const KEY_SCOPES: [&str; 2] = ["speak", "transcribe"];

/// The marketplace Claude Code installs Super Voice's plugin from, unless
/// [`CLAUDE_MARKETPLACE_VAR`] names another.
pub const CLAUDE_MARKETPLACE: &str = "super-libre/super-voice";

/// The variable, after the product's prefix (`SUPER_VOICE_CLAUDE_MARKETPLACE`),
/// that names another marketplace to install the plugin from: anything
/// `claude plugin marketplace add` takes, like a checkout's directory, to try
/// a plugin that is not on GitHub yet. That marketplace must be named
/// `super-voice` too, as [`CLAUDE_PLUGIN`] says.
pub const CLAUDE_MARKETPLACE_VAR: &str = "CLAUDE_MARKETPLACE";

/// Super Voice's plugin, as Claude Code names it: the plugin, at the
/// marketplace.
pub const CLAUDE_PLUGIN: &str = "super-voice@super-voice";

/// How long a `claude plugin` command may run. Adding the marketplace clones
/// it from GitHub.
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(120);

/// Why an assistant could not be connected or disconnected, or a key made or
/// revoked.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The daemon found no sign of the assistant on this computer.
    NotFound,
    /// No key made by hand has that id.
    UnknownKey,
    /// A key needs a name.
    NameRequired,
    /// The name is an assistant's, whose key the daemon names itself.
    NameReserved,
    /// The assistant's config could not be changed.
    Config(String),
    /// A `claude plugin` command failed.
    Plugin(String),
    /// The secrets file could not be written.
    Key(String),
}

/// The daemon's hold on the assistants: one change at a time, the tool calls
/// in flight on `/v1/mcp`, and the sessions they come from.
#[derive(Default)]
pub struct Assistants {
    /// Held across a whole connect or disconnect, so two at once cannot each
    /// mint a key for the same assistant.
    changing: tokio::sync::Mutex<()>,
    /// The tool calls in flight on `/v1/mcp`.
    pub calls: std::sync::Arc<calls::Calls>,
    /// The sessions the calls come from.
    pub sessions: std::sync::Arc<sessions::Sessions>,
}

impl Assistants {
    /// Held across a whole change.
    pub async fn changing(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.changing.lock().await
    }
}

/// The program an assistant runs as.
#[must_use]
pub fn program_name(id: AssistantId) -> &'static str {
    match id {
        AssistantId::ClaudeCode => "claude",
        AssistantId::Codex => "codex",
        AssistantId::OpenCode => "opencode",
        AssistantId::Antigravity => "agy",
    }
}

/// Directories under the home directory that assistants are installed in and
/// that a service's `PATH` usually lacks: their own installers', npm's, Bun's,
/// pnpm's and Volta's.
const HOME_BIN_DIRS: [&str; 6] = [
    ".local/bin",
    ".npm-global/bin",
    ".bun/bin",
    ".claude/local",
    ".local/share/pnpm",
    ".volta/bin",
];

/// The same, outside the home directory: Homebrew's, and the usual local
/// prefix.
const SYSTEM_BIN_DIRS: [&str; 3] = [
    "/home/linuxbrew/.linuxbrew/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
];

/// Where the daemon looks for an assistant's program: `PATH`, then the
/// directories assistants are installed in.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map_or_else(Vec::new, |path| std::env::split_paths(&path).collect());
    if let Some(home) = dirs::home_dir() {
        dirs.extend(HOME_BIN_DIRS.iter().map(|dir| home.join(dir)));
    }
    dirs.extend(SYSTEM_BIN_DIRS.iter().map(PathBuf::from));
    dirs
}

/// The assistant's program, if the daemon finds it.
#[must_use]
pub fn find_program(id: AssistantId) -> Option<PathBuf> {
    let name = program_name(id);
    search_dirs()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// Claude Code's own directory: `$CLAUDE_CONFIG_DIR`, or `~/.claude`.
fn claude_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
}

/// The directory the assistant keeps its config in, which is there once it
/// has run.
fn config_dir(id: AssistantId) -> Option<PathBuf> {
    match id {
        AssistantId::ClaudeCode => claude_dir(),
        AssistantId::Codex => assistant_configs::codex_file()?
            .parent()
            .map(Path::to_path_buf),
        AssistantId::OpenCode => {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|config| !config.is_empty())
                .map(PathBuf::from)
                .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;
            Some(config.join("opencode"))
        }
        // Its own: `~/.gemini` is the Gemini CLI's too, and the config's
        // folder the Antigravity App's.
        AssistantId::Antigravity => {
            dirs::home_dir().map(|home| home.join(".gemini/antigravity-cli"))
        }
    }
}

/// Whether the daemon finds the assistant on this computer: its program, or
/// its config.
#[must_use]
pub fn is_found(id: AssistantId) -> bool {
    find_program(id).is_some() || config_dir(id).is_some_and(|dir| dir.is_dir())
}

/// Whether Claude Code's settings, `text`, have Super Voice's plugin
/// installed. Claude Code lists every plugin installed in a scope under
/// `enabledPlugins` in that scope's settings, with whether it is on.
fn claude_has_plugin(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok_and(|settings| {
        settings
            .get("enabledPlugins")
            .and_then(|plugins| plugins.get(CLAUDE_PLUGIN))
            .is_some()
    })
}

/// Whether the assistant's config has Super Voice in it: Claude Code's user
/// settings the plugin, the others their entry.
#[must_use]
pub fn has_entry(id: AssistantId) -> bool {
    let (file, has): (Option<PathBuf>, fn(&str) -> bool) = match id {
        AssistantId::ClaudeCode => (
            claude_dir().map(|dir| dir.join("settings.json")),
            claude_has_plugin,
        ),
        AssistantId::Codex => (
            assistant_configs::codex_file(),
            assistant_configs::codex_has_super_voice,
        ),
        AssistantId::OpenCode => (
            assistant_configs::opencode_file(),
            assistant_configs::opencode_has_super_voice,
        ),
        AssistantId::Antigravity => (
            assistant_configs::antigravity_file(),
            assistant_configs::antigravity_has_super_voice,
        ),
    };
    file.and_then(|file| assistant_configs::read(&file).ok())
        .is_some_and(|text| has(&text))
}

/// Where assistant `id` stands, given whether the daemon holds a key for it.
/// Connected takes both the key and the entry: either alone does nothing.
#[must_use]
pub fn state(id: AssistantId, has_key: bool) -> AssistantState {
    if has_key && has_entry(id) {
        AssistantState::Connected
    } else if is_found(id) {
        AssistantState::Found
    } else {
        AssistantState::NotFound
    }
}

/// Write Super Voice into the config of `id`, one of the assistants with a
/// config file: the server at `url`, with `key`, or for Codex the command
/// that prints it.
///
/// # Errors
///
/// [`Error::Config`] when the file cannot be read, parsed or written.
pub fn write_entry(id: AssistantId, url: &str, key: &str) -> Result<(), Error> {
    let changed = match id {
        AssistantId::ClaudeCode => return Ok(()),
        AssistantId::Codex => {
            let helper =
                assistant_configs::codex_headers_helper(&crate::shortcuts::voice_program());
            assistant_configs::update(&config_file(id)?, |text| {
                assistant_configs::codex_with_super_voice(text, url, &helper)
            })
        }
        AssistantId::OpenCode => assistant_configs::update(&config_file(id)?, |text| {
            assistant_configs::opencode_with_super_voice(text, url, key)
        }),
        AssistantId::Antigravity => assistant_configs::update(&config_file(id)?, |text| {
            assistant_configs::antigravity_with_super_voice(text, url, key)
        }),
    };
    match changed {
        Ok(true) => info!("Wrote Super Voice into {}'s config", id.display_name()),
        Ok(false) => info!("{}'s config already had Super Voice", id.display_name()),
        Err(e) => return Err(Error::Config(e.to_string())),
    }
    Ok(())
}

/// Take Super Voice out of the config of `id`, one of the assistants with a
/// config file. One without it is left as it is.
///
/// # Errors
///
/// [`Error::Config`] when the file cannot be read, parsed or written.
pub fn remove_entry(id: AssistantId) -> Result<(), Error> {
    let without: fn(&str) -> Result<String, assistant_configs::Error> = match id {
        AssistantId::ClaudeCode => return Ok(()),
        AssistantId::Codex => assistant_configs::codex_without_super_voice,
        AssistantId::OpenCode => assistant_configs::opencode_without_super_voice,
        AssistantId::Antigravity => assistant_configs::antigravity_without_super_voice,
    };
    match assistant_configs::update(&config_file(id)?, without) {
        Ok(true) => info!("Took Super Voice out of {}'s config", id.display_name()),
        Ok(false) => {}
        Err(e) => return Err(Error::Config(e.to_string())),
    }
    Ok(())
}

/// The config file of `id`.
fn config_file(id: AssistantId) -> Result<PathBuf, Error> {
    let file = match id {
        AssistantId::ClaudeCode => claude_dir().map(|dir| dir.join("settings.json")),
        AssistantId::Codex => assistant_configs::codex_file(),
        AssistantId::OpenCode => assistant_configs::opencode_file(),
        AssistantId::Antigravity => assistant_configs::antigravity_file(),
    };
    file.ok_or_else(|| Error::Config("there is no home directory".to_string()))
}

/// The marketplace to install the plugin from: `set`, the value of
/// [`CLAUDE_MARKETPLACE_VAR`], unless it is missing or blank, else
/// [`CLAUDE_MARKETPLACE`].
fn claude_marketplace(set: Option<String>) -> String {
    set.filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| CLAUDE_MARKETPLACE.to_string())
}

/// Install Super Voice's plugin in Claude Code, with `claude`, the program
/// found: add its marketplace ([`claude_marketplace`]), then install it, for
/// the user. Either already done is fine.
///
/// # Errors
///
/// [`Error::Plugin`] when a command fails or runs too long.
pub async fn install_claude_plugin(claude: &Path) -> Result<(), Error> {
    let var = SUPER_VOICE.env(CLAUDE_MARKETPLACE_VAR);
    let marketplace = claude_marketplace(std::env::var(&var).ok());
    if marketplace != CLAUDE_MARKETPLACE {
        info!("Installing Claude Code's plugin from {marketplace}, as {var} says");
    }
    run_claude(
        claude,
        &[
            "plugin",
            "marketplace",
            "add",
            &marketplace,
            "--scope",
            "user",
        ],
        &["already"],
    )
    .await?;
    run_claude(
        claude,
        &["plugin", "install", CLAUDE_PLUGIN, "--scope", "user"],
        &["already"],
    )
    .await
}

/// Uninstall Super Voice's plugin from Claude Code, for the user. Not
/// installed is fine. The marketplace stays: the user may use it for more.
///
/// # Errors
///
/// [`Error::Plugin`] when the command fails or runs too long.
pub async fn uninstall_claude_plugin(claude: &Path) -> Result<(), Error> {
    run_claude(
        claude,
        &["plugin", "uninstall", CLAUDE_PLUGIN, "--scope", "user"],
        &["not installed", "not found"],
    )
    .await
}

/// Run `claude` with `args`. A command that fails saying one of `fine` (in
/// any case) has nothing left to do, and counts as done.
async fn run_claude(claude: &Path, args: &[&str], fine: &[&str]) -> Result<(), Error> {
    let shown = format!("claude {}", args.join(" "));
    info!("Running {shown}");
    let mut command = tokio::process::Command::new(claude);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    // A program installed with npm runs `node` through `env`, and a service's
    // `PATH` may not reach it.
    if let Some(path) = child_path(claude) {
        command.env("PATH", path);
    }
    let output = match tokio::time::timeout(CLAUDE_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(Error::Plugin(format!("could not run {shown}: {e}"))),
        Err(_) => {
            return Err(Error::Plugin(format!(
                "{shown} ran for more than {} seconds",
                CLAUDE_TIMEOUT.as_secs()
            )));
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let said = format!("{} {}", stderr.trim(), stdout.trim()).to_lowercase();
    if fine.iter().any(|phrase| said.contains(phrase)) {
        info!("{shown}: nothing to do ({})", said.trim());
        return Ok(());
    }
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    warn!("{shown} failed ({}): {detail}", output.status);
    Err(Error::Plugin(format!(
        "{shown} failed ({}): {detail}",
        output.status
    )))
}

/// `PATH` for a child of `program`: its own directory, the daemon's `PATH`,
/// and the directories assistants are installed in.
fn child_path(program: &Path) -> Option<OsString> {
    let own = program.parent().map(Path::to_path_buf);
    std::env::join_paths(own.into_iter().chain(search_dirs())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_lists_the_plugin_whether_or_not_it_is_on() {
        assert!(claude_has_plugin(
            r#"{"enabledPlugins": {"super-voice@super-voice": true}}"#
        ));
        assert!(claude_has_plugin(
            r#"{"enabledPlugins": {"super-voice@super-voice": false}}"#
        ));
        assert!(!claude_has_plugin(
            r#"{"enabledPlugins": {"super-voice@elsewhere": true}}"#
        ));
        assert!(!claude_has_plugin(r#"{"theme": "dark"}"#));
        assert!(!claude_has_plugin(""));
        assert!(!claude_has_plugin("{not json"));
    }

    #[test]
    fn each_assistant_has_its_own_program() {
        let names: Vec<&str> = AssistantId::ALL
            .iter()
            .map(|id| program_name(*id))
            .collect();
        assert_eq!(names, ["claude", "codex", "opencode", "agy"]);
    }

    #[test]
    fn the_plugin_comes_from_github_unless_the_variable_names_elsewhere() {
        assert_eq!(claude_marketplace(None), CLAUDE_MARKETPLACE);
        assert_eq!(claude_marketplace(Some(String::new())), CLAUDE_MARKETPLACE);
        assert_eq!(claude_marketplace(Some("  ".into())), CLAUDE_MARKETPLACE);
        assert_eq!(
            claude_marketplace(Some("/home/me/super-voice".into())),
            "/home/me/super-voice"
        );
        assert_eq!(
            SUPER_VOICE.env(CLAUDE_MARKETPLACE_VAR),
            "SUPER_VOICE_CLAUDE_MARKETPLACE"
        );
    }

    #[test]
    fn a_child_reaches_its_own_directory_first() {
        let path = child_path(Path::new("/opt/tools/bin/claude")).expect("joins");
        let first = std::env::split_paths(&path).next();
        assert_eq!(first, Some(PathBuf::from("/opt/tools/bin")));
    }

    /// A key grants only scopes the daemon knows.
    #[test]
    fn keys_grant_only_speaking_and_hearing() {
        for scope in KEY_SCOPES {
            assert!(
                super_voice_shared::daemon::scopes::is_known_scope(scope),
                "{scope}"
            );
        }
    }
}
