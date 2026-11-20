# Super Voice for Claude Code

A Claude Code plugin that talks to Claude Code through Super Voice, on your own
machine. Nothing leaves it.

1. Claude's replies are read aloud when it finishes. Code blocks are said as
   "code block".
2. Claude says when it needs your approval.
3. Sending your next prompt stops whatever it is reading.
4. Claude gets two tools: `speak`, and `listen` (asks you something aloud and
   hears your answer).
5. `/super-voice:talk` starts a spoken conversation: Claude listens, works,
   answers aloud and listens again until you say you are done.
6. A row above the prompt, on the key `0` in an empty prompt:
   1. `Talk` records you and puts your words in the prompt box. They are sent
      after 5 seconds unless you edit them, and Enter sends them at once.
   2. While Claude speaks, `Stop and talk` cuts it off and listens.
   3. After Claude answers something you said, the microphone opens by itself.
   4. What Claude's `listen` hears goes through the prompt box the same way.
7. A 🔊 under each reply reads it again; press it again to stop. In a terminal
   it takes a mouse click, in fullscreen mode (`/tui fullscreen`).

To type a prompt by voice, use Super Voice's own shortcut (`voice record --write`):
it types into the focused terminal like any other window.

## Requirements

1. Super Voice installed, with the daemon running and a model selected for each
   feature.
2. `voice` on your `PATH`. Super Voice's installer puts it there. The hooks use
   it, and so does the plugin to hand Claude Code its key.
3. `jq`, which the hooks use to read Claude's reply.

Claude Code reaches Super Voice at `http://127.0.0.1:7300/v1/mcp`, with a key
only Claude Code has. Disconnecting Claude Code on the Assistants page takes it
away.

Claude Code also asks before each tool call. For `/super-voice:talk` to be
hands-free, allow `listen` and `speak` without asking when it first does.

## Settings

"Ask questions aloud" is a switch on Claude Code's row on the Assistants page.
When it is on, Claude asks you its questions out loud and hears your answer,
without you asking to talk.

## Install

In the Super Voice app, open Assistants and press Connect next to Claude Code.
Super Voice installs this plugin and gives Claude Code its key. Restart Claude
Code to start using it.

To try it from a checkout, for one session only, connect Claude Code in the app
first so it has a key, then:

```
claude --plugin-dir ./claude-code-plugin
```
