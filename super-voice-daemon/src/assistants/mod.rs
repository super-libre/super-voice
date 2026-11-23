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

/// How long an assistant's plugin command may run. Adding Claude Code's
/// marketplace clones it from GitHub.
const PLUGIN_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Super Voice's plugin for Codex, as Codex names it: the plugin, at the
/// marketplace.
pub const CODEX_PLUGIN: &str = "super-voice@super-voice";

/// The marketplace Codex installs Super Voice's plugin from.
const CODEX_MARKETPLACE: &str = "super-voice";

/// Super Voice's plugin for `OpenCode`, by each file's name in its folder:
/// the server part, which holds the connection to Super Voice, and the TUI
/// part, Talk.
const OPENCODE_PLUGIN_FILES: [(&str, &str); 5] = [
    (
        "server.ts",
        include_str!("../../../opencode-plugin/server.ts"),
    ),
    ("tui.tsx", include_str!("../../../opencode-plugin/tui.tsx")),
    ("rpc.ts", include_str!("../../../opencode-plugin/rpc.ts")),
    ("mcp.ts", include_str!("../../../opencode-plugin/mcp.ts")),
    (
        "terms.ts",
        include_str!("../../../opencode-plugin/terms.ts"),
    ),
];

/// Super Voice's plugin for the Antigravity CLI, by each file's name in its
/// folder. `{voice}` in its hooks becomes the `voice` command's path.
const ANTIGRAVITY_PLUGIN_FILES: [(&str, &str); 3] = [
    (
        "plugin.json",
        include_str!("../../../antigravity-plugin/plugin.json"),
    ),
    (
        "hooks.json",
        include_str!("../../../antigravity-plugin/hooks.json"),
    ),
    (
        "skills/read/SKILL.md",
        include_str!("../../../antigravity-plugin/skills/read/SKILL.md"),
    ),
];

/// Super Voice's plugin, as the Antigravity CLI names it.
const ANTIGRAVITY_PLUGIN: &str = "super-voice";

/// Super Voice's plugin for Codex, as a marketplace of one, by each file's
/// place in it. The daemon writes it to [`codex_marketplace_dir`] and
/// installs the plugin from there.
const CODEX_MARKETPLACE_FILES: [(&str, &str); 4] = [
    (
        ".agents/plugins/marketplace.json",
        include_str!("../../../codex-plugin/.agents/plugins/marketplace.json"),
    ),
    (
        "plugins/super-voice/.codex-plugin/plugin.json",
        include_str!("../../../codex-plugin/plugins/super-voice/.codex-plugin/plugin.json"),
    ),
    (
        "plugins/super-voice/hooks/hooks.json",
        include_str!("../../../codex-plugin/plugins/super-voice/hooks/hooks.json"),
    ),
    (
        "plugins/super-voice/skills/read/SKILL.md",
        include_str!("../../../codex-plugin/plugins/super-voice/skills/read/SKILL.md"),
    ),
];

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
    /// An assistant's plugin could not be installed or uninstalled.
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
        dirs.extend(node_version_dirs(&home));
    }
    dirs.extend(SYSTEM_BIN_DIRS.iter().map(PathBuf::from));
    dirs
}

/// The directories a Node version manager under `home` puts the programs
/// npm installs in, which a service's `PATH` never has: asdf's and mise's
/// shims, fnm's default version, and each of nvm's versions, the newest
/// first.
fn node_version_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".asdf/shims"),
        home.join(".local/share/mise/shims"),
        home.join(".local/share/fnm/aliases/default/bin"),
        home.join(".fnm/aliases/default/bin"),
    ];
    let mut nvm: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(home.join(".nvm/versions/node"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name
                .trim_start_matches('v')
                .split('.')
                .map(|part| part.parse().unwrap_or(0))
                .collect();
            (version, entry.path().join("bin"))
        })
        .collect();
    nvm.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.extend(nvm.into_iter().map(|(_, dir)| dir));
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
/// its config. Claude Code only by its program, which connecting it runs:
/// found by its config alone, every connect would fail.
#[must_use]
pub fn is_found(id: AssistantId) -> bool {
    find_program(id).is_some()
        || (id != AssistantId::ClaudeCode && config_dir(id).is_some_and(|dir| dir.is_dir()))
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
        // The key is in the file itself, so only its owner may read it.
        AssistantId::OpenCode => assistant_configs::update_holding_key(&config_file(id)?, |text| {
            assistant_configs::opencode_with_super_voice(text, url, key)
        }),
        AssistantId::Antigravity => {
            assistant_configs::update_holding_key(&config_file(id)?, |text| {
                assistant_configs::antigravity_with_super_voice(text, url, key)
            })
        }
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
/// the user, set to reach Super Voice at `address`. Either already done is
/// fine: installing it again sets the address all the same.
///
/// # Errors
///
/// [`Error::Plugin`] when a command fails or runs too long.
pub async fn install_claude_plugin(claude: &Path, address: &str) -> Result<(), Error> {
    let var = SUPER_VOICE.env(CLAUDE_MARKETPLACE_VAR);
    let marketplace = claude_marketplace(std::env::var(&var).ok());
    if marketplace != CLAUDE_MARKETPLACE {
        info!("Installing Claude Code's plugin from {marketplace}, as {var} says");
    }
    run_plugin_command(
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
    let address = format!("address={address}");
    run_plugin_command(
        claude,
        &[
            "plugin",
            "install",
            CLAUDE_PLUGIN,
            "--scope",
            "user",
            "--config",
            &address,
        ],
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
    run_plugin_command(
        claude,
        &["plugin", "uninstall", CLAUDE_PLUGIN, "--scope", "user"],
        &["not installed", "not found"],
    )
    .await
}

/// Where `OpenCode` finds Super Voice's plugin: `plugins/super-voice` in its
/// global config folder, which it loads plugins from by itself.
fn opencode_plugin_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|config| !config.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;
    Some(config.join("opencode").join("plugins").join("super-voice"))
}

/// Install Super Voice's plugin in `OpenCode`: write its files to its
/// plugin folder ([`opencode_plugin_dir`]), which `OpenCode` loads, and
/// reloads when they change. The plugin reaches Super Voice with the address
/// and key in `OpenCode`'s own entry.
///
/// # Errors
///
/// [`Error::Plugin`] when the files cannot be written.
pub fn install_opencode_plugin() -> Result<(), Error> {
    let dir = opencode_plugin_dir()
        .ok_or_else(|| Error::Plugin("no home folder for OpenCode's plugin".to_string()))?;
    write_plugin_files(&dir, &OPENCODE_PLUGIN_FILES)
        .map_err(|e| Error::Plugin(format!("could not write OpenCode's plugin: {e}")))?;
    info!("Wrote Super Voice's plugin into {}", dir.display());
    Ok(())
}

/// Delete Super Voice's plugin from `OpenCode`'s plugin folder. Not there is
/// fine.
///
/// # Errors
///
/// [`Error::Plugin`] when the folder is there and cannot be deleted.
pub fn uninstall_opencode_plugin() -> Result<(), Error> {
    let Some(dir) = opencode_plugin_dir() else {
        return Ok(());
    };
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {
            info!("Deleted Super Voice's plugin from {}", dir.display());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Plugin(format!(
            "could not delete OpenCode's plugin: {e}"
        ))),
    }
}

/// Where Super Voice writes its plugin for the Antigravity CLI, which `agy
/// plugin install` copies from.
fn antigravity_plugin_dir() -> PathBuf {
    super_voice_shared::paths::data_dir()
        .join("assistants")
        .join("antigravity")
        .join(ANTIGRAVITY_PLUGIN)
}

/// Install Super Voice's plugin in the Antigravity CLI, with `agy`, the
/// program found: write it ([`antigravity_plugin_dir`]), its hooks running
/// this daemon's `voice`, and install it, which replaces an older copy.
///
/// # Errors
///
/// [`Error::Plugin`] when the files cannot be written, or the command fails
/// or runs too long.
pub async fn install_antigravity_plugin(agy: &Path) -> Result<(), Error> {
    let dir = antigravity_plugin_dir();
    let voice = assistant_configs::shell_quote(&hook_program());
    let files: Vec<(&'static str, String)> = ANTIGRAVITY_PLUGIN_FILES
        .iter()
        .map(|(name, text)| (*name, text.replace("{voice}", &voice)))
        .collect();
    let written = dir.clone();
    tokio::task::spawn_blocking(move || {
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (*name, text.as_str()))
            .collect();
        write_plugin_files(&written, &files)
    })
    .await
    .map_err(|e| Error::Plugin(e.to_string()))?
    .map_err(|e| Error::Plugin(format!("could not write the Antigravity CLI's plugin: {e}")))?;
    let source = dir.to_string_lossy();
    run_plugin_command(agy, &["plugin", "install", &source], &[]).await
}

/// The `voice` an assistant's hook runs: the one beside the daemon, under
/// either name, since a build tree names it as its crate does, or the one on
/// `PATH`.
fn hook_program() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            let dir = exe.parent()?.to_path_buf();
            ["voice", "super-voice-cli"]
                .into_iter()
                .map(|name| dir.join(name))
                .find(|voice| voice.is_file())
        })
        .map_or_else(|| "voice".to_string(), |voice| voice.display().to_string())
}

/// Uninstall Super Voice's plugin from the Antigravity CLI, and delete the
/// copy it was installed from. Not installed is fine.
///
/// # Errors
///
/// [`Error::Plugin`] when the command fails or runs too long.
pub async fn uninstall_antigravity_plugin(agy: &Path) -> Result<(), Error> {
    run_plugin_command(
        agy,
        &["plugin", "uninstall", ANTIGRAVITY_PLUGIN],
        &["not installed", "not found"],
    )
    .await?;
    let dir = antigravity_plugin_dir();
    let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(dir)).await;
    Ok(())
}

/// Where Super Voice keeps its plugin for Codex, as a marketplace.
fn codex_marketplace_dir() -> PathBuf {
    super_voice_shared::paths::data_dir()
        .join("assistants")
        .join("codex")
}

/// Write `files`, each by its place under `dir`, into `dir`. A plugin
/// manifest's `version` becomes Super Voice's own, so an assistant that keeps
/// a plugin per version takes this release's.
fn write_plugin_files(dir: &Path, files: &[(&str, &str)]) -> std::io::Result<()> {
    for (place, text) in files {
        let path = dir.join(place);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = if place.ends_with("plugin.json") {
            with_version(text, env!("CARGO_PKG_VERSION"))
        } else {
            (*text).to_string()
        };
        std::fs::write(&path, text)?;
    }
    Ok(())
}

/// A plugin manifest, `text`, with `version`.
fn with_version(text: &str, version: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(mut manifest) => {
            manifest["version"] = version.into();
            serde_json::to_string_pretty(&manifest).map_or_else(|_| text.to_string(), |t| t + "\n")
        }
        Err(_) => text.to_string(),
    }
}

/// Install Super Voice's plugin in Codex, with `codex`, the program found:
/// write its marketplace ([`codex_marketplace_dir`]), add it, and install the
/// plugin from it. Codex runs the plugin's hooks once the user trusts them,
/// in its `/hooks`.
///
/// # Errors
///
/// [`Error::Plugin`] when the files cannot be written, or a command fails or
/// runs too long.
pub async fn install_codex_plugin(codex: &Path) -> Result<(), Error> {
    let dir = codex_marketplace_dir();
    let written = dir.clone();
    tokio::task::spawn_blocking(move || write_plugin_files(&written, &CODEX_MARKETPLACE_FILES))
        .await
        .map_err(|e| Error::Plugin(e.to_string()))?
        .map_err(|e| Error::Plugin(format!("could not write Codex's plugin: {e}")))?;
    let source = dir.to_string_lossy();
    run_plugin_command(
        codex,
        &["plugin", "marketplace", "add", &source],
        &["already"],
    )
    .await?;
    // A plugin already there may be an older release's: it is put in again.
    run_plugin_command(
        codex,
        &["plugin", "remove", CODEX_PLUGIN],
        &["not installed", "not found", "no plugin"],
    )
    .await?;
    run_plugin_command(codex, &["plugin", "add", CODEX_PLUGIN], &["already"]).await
}

/// Uninstall Super Voice's plugin from Codex, take its marketplace out, and
/// delete the marketplace's files. Not installed is fine.
///
/// # Errors
///
/// [`Error::Plugin`] when a command fails or runs too long.
pub async fn uninstall_codex_plugin(codex: &Path) -> Result<(), Error> {
    run_plugin_command(
        codex,
        &["plugin", "remove", CODEX_PLUGIN],
        &["not installed", "not found", "no plugin"],
    )
    .await?;
    run_plugin_command(
        codex,
        &["plugin", "marketplace", "remove", CODEX_MARKETPLACE],
        &["not found", "no marketplace", "not configured"],
    )
    .await?;
    let dir = codex_marketplace_dir();
    let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(dir)).await;
    Ok(())
}

/// Run an assistant's `program` with `args`. A command that fails saying one
/// of `fine` (in any case) has nothing left to do, and counts as done.
async fn run_plugin_command(program: &Path, args: &[&str], fine: &[&str]) -> Result<(), Error> {
    let name = program
        .file_name()
        .map_or_else(|| program.to_string_lossy(), |name| name.to_string_lossy());
    let shown = format!("{name} {}", args.join(" "));
    info!("Running {shown}");
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    // A program installed with npm runs `node` through `env`, and a service's
    // `PATH` may not reach it.
    if let Some(path) = child_path(program) {
        command.env("PATH", path);
    }
    let output = match tokio::time::timeout(PLUGIN_COMMAND_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(Error::Plugin(format!("could not run {shown}: {e}"))),
        Err(_) => {
            return Err(Error::Plugin(format!(
                "{shown} ran for more than {} seconds",
                PLUGIN_COMMAND_TIMEOUT.as_secs()
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

    /// The Node version managers' directories are searched, nvm's newest
    /// version first, numbers compared as numbers.
    #[test]
    fn node_version_managers_are_searched() {
        let home = tempfile::tempdir().expect("home");
        for version in ["v9.11.2", "v20.11.1", "v20.9.0"] {
            std::fs::create_dir_all(
                home.path()
                    .join(".nvm/versions/node")
                    .join(version)
                    .join("bin"),
            )
            .expect("nvm version");
        }
        let dirs = node_version_dirs(home.path());
        let nvm: Vec<_> = dirs
            .iter()
            .filter_map(|dir| {
                dir.strip_prefix(home.path().join(".nvm/versions/node"))
                    .ok()
            })
            .map(|dir| dir.to_string_lossy().into_owned())
            .collect();
        assert_eq!(nvm, ["v20.11.1/bin", "v20.9.0/bin", "v9.11.2/bin"]);
        assert!(dirs.contains(&home.path().join(".asdf/shims")));
        assert!(dirs.contains(&home.path().join(".local/share/mise/shims")));
    }

    #[test]
    fn each_assistant_has_its_own_program() {
        let names: Vec<&str> = AssistantId::ALL
            .iter()
            .map(|id| program_name(*id))
            .collect();
        assert_eq!(names, ["claude", "codex", "opencode", "agy"]);
    }

    /// A plugin's files are written with Super Voice's own version in its
    /// manifest, so an assistant that keeps one per version takes the new
    /// release's.
    #[test]
    fn plugin_files_carry_this_release() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_files(dir.path(), &CODEX_MARKETPLACE_FILES).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                dir.path()
                    .join("plugins/super-voice/.codex-plugin/plugin.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest["hooks"], "./hooks/hooks.json");
        let hooks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("plugins/super-voice/hooks/hooks.json"))
                .unwrap(),
        )
        .unwrap();
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "Stop",
            "PermissionRequest",
        ] {
            let hook = &hooks["hooks"][event][0]["hooks"][0];
            assert_eq!(hook["type"], "mcp_tool", "{event}");
            assert_eq!(hook["server"], "super-voice", "{event}");
            assert_eq!(hook["input"]["hook"], "codex", "{event}");
        }
        assert_eq!(with_version("not json", "1.0.0"), "not json");
    }

    /// OpenCode's plugin has its server and TUI parts, and every file they
    /// import.
    #[test]
    fn opencode_plugin_has_every_part() {
        let names: Vec<&str> = OPENCODE_PLUGIN_FILES
            .iter()
            .map(|(name, _)| *name)
            .collect();
        for (name, text) in OPENCODE_PLUGIN_FILES {
            assert!(!text.is_empty(), "{name}");
            for import in ["./mcp", "./rpc", "./terms"] {
                if text.contains(&format!("from '{import}'")) {
                    let file = import.trim_start_matches("./");
                    assert!(
                        names.iter().any(|name| name.starts_with(file)),
                        "{name} imports {import}"
                    );
                }
            }
        }
        assert!(OPENCODE_PLUGIN_FILES[0].1.contains("Plugin.define"));
        assert!(OPENCODE_PLUGIN_FILES[1].1.contains("session.composer.top"));
    }

    /// The Antigravity CLI's plugin runs `voice assistant-hook` for each of
    /// its hooks, the command filled in when it is written.
    #[test]
    fn antigravity_hooks_run_voice() {
        let hooks: serde_json::Value = serde_json::from_str(ANTIGRAVITY_PLUGIN_FILES[1].1).unwrap();
        let group = &hooks["super-voice"];
        let commands = [
            &group["SessionStart"][0]["command"],
            &group["PreInvocation"][0]["command"],
            &group["PreToolUse"][0]["hooks"][0]["command"],
            &group["Stop"][0]["command"],
        ];
        for (command, event) in
            commands
                .iter()
                .zip(["session-start", "pre-invocation", "pre-tool-use", "stop"])
        {
            assert_eq!(
                command.as_str(),
                Some(format!("{{voice}} assistant-hook antigravity {event}").as_str())
            );
        }
        let manifest: serde_json::Value =
            serde_json::from_str(ANTIGRAVITY_PLUGIN_FILES[0].1).unwrap();
        assert_eq!(manifest["name"], ANTIGRAVITY_PLUGIN);
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
