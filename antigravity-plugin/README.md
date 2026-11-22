# Super Voice for the Antigravity CLI

Hear the Antigravity CLI (`agy`) answer what you dictate to it, with [Super Voice](https://github.com/super-libre/super-voice) on your own machine.

Connect the Antigravity CLI on the Assistants page of the Super Voice app. Super Voice adds its MCP server to `~/.gemini/config/mcp_config.json`, and installs this plugin with `agy plugin install`, after writing the path of its `voice` command into `hooks.json` (the `{voice}` placeholder here). Disconnecting uninstalls it.

Besides its hooks, the plugin has one skill, `read` (`skills/read/SKILL.md`), only so `/read` is a command agy knows. Its hook answers it; the skill itself only tells agy to ask you to reconnect, if the hooks don't run.

agy's hooks can only run a command, so each one runs `voice assistant-hook antigravity <event>`, which tells Super Voice and answers for it:

1. **SessionStart:** Super Voice lists the conversation, with its folder.
2. **PreInvocation:** a prompt that holds what you just dictated with Super Voice's shortcut counts as spoken, and agy is told to answer it briefly, to be heard. A prompt you typed is answered in writing. A prompt of **/read** reads agy's last reply aloud again, and **/read 1** the one before it, up to **/read 9**; **/read** while Super Voice reads stops it. Super Voice answers it, and the turn ends before the model runs. The command is the plugin's `read` skill, which agy names `/super-voice:read`: typing `/read` offers it. When it works you hear it, and nothing is shown. When it can't, like with no reply to read yet, it says why with a ⚠: agy shows a hook's text to you alone only as an error.
3. **PreToolUse:** the model's `speak` and `listen` calls are named as this conversation's, so Super Voice refuses them in a typed turn.
4. **Stop:** Super Voice keeps the answer, the last 10 of each conversation, for `/read`. The answer to a spoken prompt is read aloud.

agy has no hook for a permission request, and no way for a plugin to add a key, so it doesn't announce, and has no Talk: dictate your prompt with Super Voice's shortcut instead.
