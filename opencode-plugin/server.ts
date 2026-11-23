// SPDX-License-Identifier: GPL-3.0-only
//
// Super Voice's OpenCode plugin, the server part. It holds the connection to
// Super Voice and does what the TUI part asks; tells Super Voice of each
// session, and whether each prompt was spoken; keeps the plugin's own tools
// from the model; lets the model speak and listen only in a spoken turn, and
// tells it so; reads a spoken prompt's reply aloud, and any reply again when
// the user asks with /read; and says aloud when
// OpenCode needs the user's permission.

import { Plugin } from '@opencode/plugin'

import { SuperVoice, type Target } from './mcp'
import { type ReadInput, type SessionInput, type Status, Voice } from './rpc'
import { talkArgs } from './terms'
import { MODEL_TOOL_NAMES, NOT_SPOKEN_TO, PLUGIN_TOOL_NAMES, SPOKEN_NOTE, fieldsOf } from './voice'

/** Super Voice's entry in OpenCode's config, which names its tools. */
const SERVER = 'super-voice'
/** The tools the model calls, as OpenCode names them. */
const VOICE_TOOLS: ReadonlySet<string> = new Set(MODEL_TOOL_NAMES.map(tool => `${SERVER}_${tool}`))
/** The tools this plugin calls itself, kept from the model. */
const PLUGIN_TOOLS = PLUGIN_TOOL_NAMES.map(tool => `${SERVER}_${tool}`)

/** Whether a stored user message was sent by Talk. */
function isSpoken(metadata: unknown): boolean {
  const voice = (metadata as { superVoice?: { spoken?: unknown } } | undefined)?.superVoice
  return voice?.spoken === true
}

/** The text of a stored message, its text parts joined. */
function textOf(message: unknown): string {
  const { text, content } = message as { text?: unknown; content?: unknown }
  if (typeof text === 'string') {
    return text
  }
  if (!Array.isArray(content)) {
    return ''
  }
  return content
    .map(part => {
      const { type, text } = part as { type?: unknown; text?: unknown }
      return type === 'text' && typeof text === 'string' ? text : ''
    })
    .join('')
}

/** Why `/read` found no reply that far back, with `count` replies to read. */
function tooFarBack(count: number): string {
  if (count === 0) {
    return 'There is no reply to read yet.'
  }
  return count === 1 ? 'There is only one reply to read: 0.' : `There are ${count} replies to read: 0 to ${count - 1}.`
}

/** Whether a stored message is the user's, or the assistant's. */
function roleOf(message: unknown): string {
  const { type, role } = message as { type?: unknown; role?: unknown }
  return typeof role === 'string' ? role : typeof type === 'string' ? type : ''
}

export default Plugin.define({
  id: 'super-voice',
  async setup(ctx) {
    const folder = ctx.location.directory
    // Where Super Voice is and the key it gave OpenCode: OpenCode's own entry.
    let target: Target | undefined
    const reading = await ctx.mcp.transform(editor => {
      const entry = editor.get(SERVER) as { url?: unknown; headers?: unknown } | undefined
      target =
        typeof entry?.url === 'string'
          ? { url: entry.url, headers: (entry.headers as Record<string, string> | undefined) ?? {} }
          : undefined
    })
    const voice = new SuperVoice(() => target)
    /** Each session's turn under way: whether it was spoken. */
    const spoken = new Map<string, boolean>()

    const session = (sessionID: string) => ({ id: sessionID, folder })
    const call = (tool: string, sessionID: string, args: Record<string, unknown> = {}, signal?: AbortSignal) =>
      voice.call(tool, { ...args, session: session(sessionID) }, signal)

    /** The session's turns, oldest first: the prompt that started each, and
     * its reply, what the assistant last wrote in it. */
    const turnsOf = async (sessionID: string) => {
      const messages = ((await ctx.session.context({ sessionID }).catch(() => [])) ?? []) as unknown[]
      const turns: Array<{ prompt: string; reply: string }> = []
      for (const message of messages) {
        const text = textOf(message).trim()
        const role = roleOf(message)
        if (!text || (role !== 'user' && role !== 'assistant')) {
          continue
        }
        if (role === 'user' || turns.length === 0) {
          turns.push({ prompt: role === 'user' ? text : '', reply: '' })
        }
        if (role === 'assistant') {
          turns[turns.length - 1].reply = text
        }
      }
      return turns
    }
    /** The last turn's prompt and reply, for what Talk listens for and the
     * reply to read. */
    const lastExchange = async (sessionID: string) =>
      (await turnsOf(sessionID)).at(-1) ?? { prompt: '', reply: '' }
    /** The session's replies, the last one first. */
    const replies = async (sessionID: string) =>
      (await turnsOf(sessionID))
        .map(turn => turn.reply)
        .filter(Boolean)
        .reverse()

    const hidden = await ctx.tool.transform(editor => {
      for (const tool of PLUGIN_TOOLS) {
        editor.remove(tool)
      }
    })

    // Talk's words are spoken, and so are ones the user dictated into the
    // prompt with Super Voice's shortcut, which Super Voice recognizes.
    const prompted = await ctx.session.hook('prompt', async event => {
      const isTalk = isSpoken(event.metadata)
      const answer = await call('turn', event.sessionID, {
        event: 'prompt',
        text: event.prompt.text,
        ...(isTalk ? { spoken: true } : {}),
      })
      spoken.set(event.sessionID, isTalk || fieldsOf(answer).spoken === true)
    })

    const told = await ctx.session.hook('context', event => {
      const lastPrompt = [...event.messages].reverse().find(message => roleOf(message) === 'user')
      const isTalk = spoken.get(event.sessionID) ?? isSpoken(lastPrompt?.metadata)
      if (isTalk) {
        event.system.push({ type: 'text', text: SPOKEN_NOTE })
      }
    })

    const guarded = await ctx.tool.hook('execute.before', event => {
      if (!VOICE_TOOLS.has(event.tool)) {
        return
      }
      if (spoken.get(event.sessionID) === false) {
        throw new Error(NOT_SPOKEN_TO)
      }
      // Named as this session, with its folder.
      event.input = { ...(event.input as Record<string, unknown>), session: session(event.sessionID) }
    })

    const rpc = await ctx.rpc.register(Voice, {
      talk: async (input, context) => {
        const { sessionID } = input as SessionInput
        const { prompt, reply } = await lastExchange(sessionID)
        const heard = await call('listen', sessionID, talkArgs(prompt, reply), context.signal)
        return heard.isError ? { error: heard.text } : { text: heard.text }
      },
      read: async input => {
        const { sessionID, back } = input as ReadInput
        const found = await replies(sessionID)
        const reply = found[back]
        if (reply === undefined) {
          return { error: tooFarBack(found.length) }
        }
        // The user asked for it: a typed turn does not refuse it.
        const spoke = await call('speak', sessionID, { text: reply, asked: true })
        return spoke.isError ? { error: spoke.text } : {}
      },
      stop: async input => {
        await call('stop', (input as SessionInput).sessionID)
        return {}
      },
      status: async input => {
        const { sessionID } = input as SessionInput
        const answer = await call('status', sessionID)
        const status = fieldsOf(answer)
        const answered: Status = {
          speaking: status.speaking === true,
          conversation: spoken.get(sessionID) === true,
          // Shown until Super Voice says the user turned it off.
          talk: status.talk !== false,
        }
        return answered
      },
    })

    const idle = await ctx.session.hook('experimental.idle', async event => {
      if (spoken.get(event.sessionID) !== true) {
        return
      }
      const { reply } = await lastExchange(event.sessionID)
      const answer = await call('turn', event.sessionID, { event: 'reply', text: reply })
      if (fieldsOf(answer).read === true) {
        await rpc.events.emit('replied', { sessionID: event.sessionID })
      }
    })

    // Sessions and permission requests come as events, from every location:
    // this instance answers for its own.
    const events = new AbortController()
    void (async () => {
      const stream = ctx.event.subscribe({
        types: ['session.created', 'session.deleted', 'permission.asked'],
        signal: events.signal,
      })
      for await (const event of stream) {
        const here = (event as { location?: { directory?: string } }).location?.directory
        if (here !== undefined && here !== folder) {
          continue
        }
        const data = event.data as { sessionID?: string; message?: string }
        if (!data.sessionID) {
          continue
        }
        if (event.type === 'session.created') {
          // OpenCode keeps a session past the window it was opened in, so it
          // is listed while it is in use rather than while it is open.
          await call('session', data.sessionID, { event: 'seen' })
        } else if (event.type === 'session.deleted') {
          spoken.delete(data.sessionID)
          await call('session', data.sessionID, { event: 'end' })
        } else {
          await call('announce', data.sessionID, { text: data.message || 'OpenCode needs your permission.' })
        }
      }
    })().catch(() => {})

    return async () => {
      events.abort()
      for (const registration of [reading, hidden, prompted, told, guarded, rpc, idle]) {
        await registration.dispose()
      }
    }
  },
})
