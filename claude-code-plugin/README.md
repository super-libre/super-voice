# Super Voice for Claude Code

A Claude Code plugin that talks to Claude Code through Super Voice, on your own
machine. Nothing leaves it.

Claude speaks only after being spoken to. You start talking with the `0` key,
and from then on:

1. Claude's reply to what you said is read aloud. Code blocks are said as "code
   block". The microphone opens after it, so you can answer.
2. Claude can use Super Voice's two tools, `speak`, and `listen` (asks you
   something aloud and hears your answer).

A prompt you type gets a silent, written reply, and Claude can't use those
tools, even if the prompt asks it to.

Whether you talk or type:

1. Claude says when it needs your permission. You can turn this off.
2. Sending your next prompt stops whatever is being read.
3. A row above the prompt, on the key `0` in an empty prompt:
   1. `Talk` records you and puts your words in the prompt box. They are sent
      after 5 seconds unless you edit them, and Enter sends them at once.
   2. While something is being read, `0` stops it. In a spoken conversation,
      `Stop and talk` also opens the microphone.
   3. What Claude's `listen` hears goes through the prompt box the same way.
      Stopping it without saying anything ends Claude's turn.
4. A 🔊 under each reply reads it again; press it again to stop. In a terminal
   it takes a mouse click, in fullscreen mode (`/tui fullscreen`).

Super Voice's own shortcut (`voice record --write`) types into the focused
terminal like any other window. A prompt typed that way counts as typed.

## Requirements

1. Super Voice installed, with the daemon running and a model selected for each
   feature.
2. `voice` on your `PATH`. Super Voice's installer puts it there. The plugin
   uses it to talk, to say when Claude needs you, and to hand Claude Code its
   key.
3. `jq`, which the hooks use to read Claude Code's notices.

Claude Code reaches Super Voice at `http://127.0.0.1:7300/v1/mcp`, with a key
only Claude Code has. Disconnecting Claude Code on the Assistants page takes it
away.

## Settings

"Announce when it needs you" is a switch on Claude Code's row on the Assistants
page, on by default. It says whether Claude Code's notices, like "Claude needs
your permission", are said aloud, in every session.

## Install

In the Super Voice app, open Assistants and press Connect next to Claude Code.
Super Voice installs this plugin and gives Claude Code its key. Restart Claude
Code to start using it.

To try it from a checkout instead, have the daemon install it from there: set
`SUPER_VOICE_CLAUDE_MARKETPLACE` to the checkout's directory in the daemon's
environment, restart the daemon, and press Connect. `set-environment` lasts
until you log out.

```
systemctl --user set-environment SUPER_VOICE_CLAUDE_MARKETPLACE=/path/to/super-voice
systemctl --user restart super-voice
```

The plugin then loads from the checkout, so edits reach Claude Code at its next
start. Connecting again without the variable goes back to the plugin on GitHub.
