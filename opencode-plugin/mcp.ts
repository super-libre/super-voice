// SPDX-License-Identifier: GPL-3.0-only
//
// A small client for Super Voice's MCP server, for the calls this plugin
// makes itself. OpenCode has no way for a plugin to call an MCP tool through
// its own connection, so the plugin keeps one of its own, with the address
// and key OpenCode's entry for Super Voice holds.

/** Where Super Voice is, and the headers that let the plugin in. */
export type Target = { url: string; headers: Record<string, string> }

import type { Answer } from './voice'

/** The answer to a call that never reached Super Voice. */
const UNREACHABLE: Answer = { text: 'Super Voice is not reachable.', isError: true }

type Message = {
  id?: number
  result?: { content?: Array<{ type?: string; text?: string }>; isError?: boolean }
  error?: { message?: string }
}

/** Why Super Voice refused a request, from its error's `error_code`: the
 * key was revoked, it may not call the tool, or it called too often. */
function refusal(status: number, body: string): string {
  if (status === 429) {
    return 'Super Voice is asked too often just now. Try again in a little while.'
  }
  if (status === 401 || status === 403) {
    return 'Super Voice refused OpenCode: connect it again on the Assistants page of the Super Voice app.'
  }
  let code: unknown
  try {
    code = (JSON.parse(body) as { error_code?: unknown }).error_code
  } catch {
    code = undefined
  }
  return `Super Voice refused the request (${typeof code === 'string' ? code : `HTTP ${status}`}).`
}

export class SuperVoice {
  private connection: string | undefined
  private next = 0

  constructor(private readonly target: () => Target | undefined) {}

  /** Call `tool` with `args`: its answer, an error when the call failed. */
  async call(tool: string, args: Record<string, unknown>, signal?: AbortSignal): Promise<Answer> {
    const target = this.target()
    if (!target) {
      return UNREACHABLE
    }
    try {
      if (this.connection === undefined) {
        await this.initialize(target, signal)
      }
      const message = await this.post(target, 'tools/call', { name: tool, arguments: args }, signal)
      if (message.error) {
        return { text: message.error.message ?? '', isError: true }
      }
      const text = (message.result?.content ?? []).map(block => (block.type === 'text' ? (block.text ?? '') : '')).join('')
      return { text, isError: message.result?.isError === true }
    } catch {
      return UNREACHABLE
    }
  }

  private async initialize(target: Target, signal?: AbortSignal) {
    this.connection = undefined
    const response = await fetch(target.url, {
      method: 'POST',
      headers: this.headers(target),
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: ++this.next,
        method: 'initialize',
        params: {
          protocolVersion: '2025-06-18',
          capabilities: {},
          clientInfo: { name: 'super-voice-opencode', version: '1' },
        },
      }),
      signal,
    })
    // Refused, the next call tries again.
    this.connection = response.ok ? (response.headers.get('mcp-session-id') ?? '') : undefined
    await response.text()
  }

  /** One request's answer. Super Voice serves a connection it does not know,
   * as after it restarted, all the same. */
  private async post(target: Target, method: string, params: unknown, signal?: AbortSignal): Promise<Message> {
    const id = ++this.next
    const response = await fetch(target.url, {
      method: 'POST',
      headers: this.headers(target),
      body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
      signal,
    })
    const body = await response.text()
    if (!response.ok) {
      return { error: { message: refusal(response.status, body) } }
    }
    const type = response.headers.get('content-type') ?? ''
    if (!type.includes('text/event-stream')) {
      return JSON.parse(body) as Message
    }
    // A stream: the answer is the event carrying this request's id.
    for (const event of body.split(/\r?\n\r?\n/)) {
      const data = event
        .split(/\r?\n/)
        .filter(line => line.startsWith('data:'))
        .map(line => line.slice(5).trimStart())
        .join('\n')
      if (!data) {
        continue
      }
      const message = JSON.parse(data) as Message
      if (message.id === id) {
        return message
      }
    }
    return { error: { message: 'Super Voice gave no answer.' } }
  }

  private headers(target: Target): Record<string, string> {
    return {
      ...target.headers,
      'content-type': 'application/json',
      accept: 'application/json, text/event-stream',
      ...(this.connection ? { 'mcp-session-id': this.connection } : {}),
    }
  }
}
