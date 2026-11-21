# Super Voice for Codex

Hear Codex answer what you say to it, with [Super Voice](https://github.com/super-libre/super-voice) on your own machine.

Connect Codex on the Assistants page of the Super Voice app. Super Voice adds its MCP server to `~/.codex/config.toml`, writes this folder as a marketplace to `~/.local/share/super-voice/assistants/codex`, and installs the plugin from it with `codex plugin add super-voice@super-voice`. Disconnecting uninstalls it.

Codex runs a plugin's hooks only once you trust them: open `/hooks` in Codex and review Super Voice's.

What the hooks do, each by calling Super Voice through Codex's own connection to it:

1. **SessionStart:** Super Voice lists the conversation, with its folder.
2. **UserPromptSubmit:** a prompt that holds what you just dictated with Super Voice's shortcut counts as spoken. Codex is told to answer it briefly, to be heard. A prompt you typed is answered in writing: Super Voice refuses to speak or listen in it.
3. **Stop:** the answer to a spoken prompt is read aloud.
4. **PermissionRequest:** "Codex needs your permission" is said aloud, unless you turned off **Announce when it needs you** for Codex.

Codex has no way for a plugin to add a key, so there's no Talk here: dictate your prompt with Super Voice's shortcut instead.
