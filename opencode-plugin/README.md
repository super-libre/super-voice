# Super Voice for OpenCode

Talk to OpenCode and hear its answers, with [Super Voice](https://github.com/super-libre/super-voice) on your own machine. For OpenCode 2.

Connect OpenCode on the Assistants page of the Super Voice app. Super Voice adds its MCP server to OpenCode's config and writes this plugin to `~/.config/opencode/plugins/super-voice`, which OpenCode loads by itself. Disconnecting deletes it.

## In OpenCode

1. A row above the prompt, and the key **0** while the prompt is empty:
   - **0: Talk** records you until you stop talking, and sends what you said.
   - **Listening. 0: Stop listening** ends the recording, which is still sent.
   - **0: Stop speaking** stops Super Voice. In a spoken conversation it's **0: Stop and talk**, and the microphone opens.
2. After a spoken prompt, OpenCode is told to answer briefly, and its answer is read aloud. Then the microphone opens by itself, unless you've started typing. A prompt is spoken when Talk heard it, or when you dictated it into OpenCode with Super Voice's shortcut and sent it within 90 seconds, even with a word or two fixed by hand.
3. A prompt you typed is answered in writing: the model's calls to `speak` and `listen` are refused in it.
4. Super Voice lists each session with its folder, and says aloud when OpenCode asks for your permission, unless you turned off **Announce when it needs you** for OpenCode.

## Files

- `server.ts`, the server part: OpenCode has no way for a plugin to call an MCP tool through its own connection, so it holds one of its own (`mcp.ts`), with the address and key from OpenCode's entry for Super Voice. It tells Super Voice of each session and prompt, keeps the plugin's own tools from the model, reads replies, and announces permission requests.
- `tui.tsx`, the TUI part: the row and the key. It asks the server part over plugin RPC (`rpc.ts`).
- `terms.ts`: the names and terms Talk listens for, picked from the last prompt and reply. It's the Claude Code plugin's, linked.
