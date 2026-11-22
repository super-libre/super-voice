// SPDX-License-Identifier: GPL-3.0-only
//
// Super Voice's OpenCode plugin, the server part. It holds the connection to
// Super Voice and does what the TUI part asks; tells Super Voice of each
// session, and whether each prompt was spoken; keeps the plugin's own tools
// from the model; lets the model speak and listen only in a spoken turn, and
// tells it so; reads a spoken prompt's reply aloud; and says aloud when
// OpenCode needs the user's permission.

import { Plugin } from '@opencode/plugin'

import { SuperVoice, type Target } from './mcp'
import { Voice } from './rpc'
import { pickTerms } from './terms'

/** Super Voice's entry in OpenCode's config, which names its tools. */
const SERVER = 'super-voice'
/** The tools the model calls, as OpenCode names them. */
const VOICE_TOOLS: ReadonlySet<string> = new Set([`${SERVER}_speak`, `${SERVER}_listen`])
/** The tools this plugin calls itself, kept from the model. */
const PLUGIN_TOOLS = ['stop', 'status', 'announce', 'session', 'turn'].map(tool => `${SERVER}_${tool}`)
/** What the model is told beside a spoken prompt. */
const SPOKEN_NOTE =
  'The user said this aloud, through Super Voice. Your reply will be read aloud to them, ' +
  'and the microphone opens after it so they can answer by voice: keep the reply short ' +
  'and plain to listen to, end with any question you have, and do not call `listen` ' +
  'to hear their answer.'
/** Why the model's call to a Super Voice tool is refused in a typed turn. */
const NOT_SPOKEN_TO =
  'The user typed this prompt, so answer in writing. Super Voice speaks and listens only ' +
  'while the user is talking to you by voice, which they start with Talk.'

/** Whether a stored user message was sent by Talk. */
function isSpoken(metadata: unknown): boolean {
  const voice = (metadata as { superVoice?: { spoken?: unknown } } | undefined)?.superVoice
  return voice?.spoken === true
}

/** Whether Super Voice's answer to a prompt's `turn` says it was spoken. */
function saysSpoken(text: string | undefined): boolean {
  try {
    return (JSON.parse(text ?? '{}') as { spoken?: unknown }).spoken === true
  } catch {
    return false
  }
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

    /** The last prompt and reply of a session, for what Talk listens for. */
    const lastExchange = async (sessionID: string) => {
      const messages = ((await ctx.session.context({ sessionID }).catch(() => [])) ?? []) as unknown[]
      const last = (role: string) => textOf([...messages].reverse().find(message => roleOf(message) === role) ?? {})
      return { prompt: last('user'), reply: last('assistant') }
    }

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
      spoken.set(event.sessionID, isTalk || saysSpoken(answer?.isError ? undefined : answer?.text))
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
        const { sessionID } = input as { sessionID: string }
        const { prompt, reply } = await lastExchange(sessionID)
        const vocabulary = pickTerms(`${reply}\n${prompt}`)
        const heard = await call(
          'listen',
          sessionID,
          { asked: true, ...(vocabulary.length > 0 ? { context: { vocabulary } } : {}) },
          context.signal,
        )
        if (heard === undefined) {
          return { error: 'Super Voice is not reachable.' }
        }
        return heard.isError ? { error: heard.text } : { text: heard.text }
      },
      stop: async input => {
        await call('stop', (input as { sessionID: string }).sessionID)
        return {}
      },
      status: async input => {
        const { sessionID } = input as { sessionID: string }
        const answer = await call('status', sessionID)
        const status = (() => {
          try {
            return JSON.parse(answer?.text ?? '{}') as { speaking?: unknown; listening?: unknown }
          } catch {
            return {}
          }
        })()
        return {
          speaking: status.speaking === true,
          listening: status.listening === true,
          conversation: spoken.get(sessionID) === true,
        }
      },
    })

    const idle = await ctx.session.hook('experimental.idle', async event => {
      if (spoken.get(event.sessionID) !== true) {
        return
      }
      const { reply } = await lastExchange(event.sessionID)
      const answer = await call('turn', event.sessionID, { event: 'reply', text: reply })
      if (answer && !answer.isError && answer.text.includes('"read":true')) {
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
