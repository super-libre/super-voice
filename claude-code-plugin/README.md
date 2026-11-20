# Super Voice for Claude Code

A Claude Code plugin that talks to Claude Code through Super Voice, on your own
machine. Nothing leaves it.

1. Claude's replies are read aloud when it finishes. Code blocks are said as
   "code block".
2. Claude says when it needs your approval.
3. Sending your next prompt stops whatever it is reading.
4. Claude gets three tools: `speak`, `listen` (asks you something aloud and hears
   your answer) and `synthesize` (saves speech to a WAV file).
5. `/super-voice:talk` starts a spoken conversation: Claude listens, works,
   answers aloud and listens again until you say you are done.

To type a prompt by voice, use Super Voice's own shortcut (`voice record --write`):
it types into the focused terminal like any other window.

## Requirements

1. Super Voice installed, with the daemon running and a model selected for each
   feature.
2. `super-voice-mcp` and `voice` on your `PATH`. From a checkout,
   `just install-mcp` installs the first.
3. `jq`, which the hooks use to read Claude's reply.

The first time Claude uses a tool, Super Voice asks whether to let
`super-voice-mcp` in. The hooks use the `voice` command, which Super Voice lets
in without asking.

Claude Code also asks before each tool call. For `/super-voice:talk` to be
hands-free, allow `listen` and `speak` without asking when it first does.

## Settings

"Ask questions aloud" is off by default. When it is on, Claude asks you its
questions out loud and hears your answer, without you asking to talk. Set it
when you install the plugin, or later with
`/plugin configure super-voice@super-voice`, then restart Claude Code.

## Install

```
/plugin marketplace add jorge-menjivar/super-voice
/plugin install super-voice@super-voice
```

To try it from a checkout, for one session only:

```
claude --plugin-dir ./claude-code-plugin
```
