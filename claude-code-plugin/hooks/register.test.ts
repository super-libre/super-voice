// SPDX-License-Identifier: GPL-3.0-only
import { expect, test } from 'claude-code/testing'

import { NOT_SPOKEN_TO, SPOKEN_NOTE } from './voice'

const SPEAK = 'mcp__plugin_super-voice_voice__speak'

/** What a call the plugin refused tells Claude. */
function refusal(result: { deny?: string; text?: string; isError?: boolean }) {
  return result.deny ?? (result.isError ? result.text : undefined)
}

test("Claude's call in a typed turn is refused", async ($, on) => {
  let isRun = false
  on('tool.call', { tool: SPEAK }, () => {
    isRun = true
    return { result: 'ok' }
  })

  const result = await $.tool.call({ tool: SPEAK, text: 'Hello.' })

  expect(refusal(result)).toBe(NOT_SPOKEN_TO)
  expect(isRun).toBe(false)
})

test("Claude's call that names a session is refused", async ($, on) => {
  let isRun = false
  on('tool.call', { tool: SPEAK }, () => {
    isRun = true
    return { result: 'ok' }
  })

  const result = await $.tool.call({
    tool: SPEAK,
    text: 'Hello.',
    session: { id: 'another' },
  } as Parameters<typeof $.tool.call>[0])

  expect(refusal(result)).toMatch(/`session` is not an argument/)
  expect(isRun).toBe(false)
})

test("Claude's call in a spoken turn goes on as Claude made it", async ($, on) => {
  on('session.id', () => ({ value: 'session-1' }))
  on('session.cwd', () => ({ value: '/home/user/project' }))
  const told: { tool: string; args: Record<string, unknown> }[] = []
  on('mcp.call', (_, e) => {
    told.push({ tool: e.tool, args: e.args })
    const answer = e.tool === 'turn' ? { spoken: true } : {}
    return { value: { content: [{ type: 'text', text: JSON.stringify(answer) }] } }
  })
  const sent: (readonly string[] | undefined)[] = []
  on('prompt.submit', (_, e) => {
    sent.push(e.context)
    return { text: e.text, context: e.context, origin: e.origin }
  })
  const ran: unknown[] = []
  on('tool.call', { tool: SPEAK }, (_, e) => {
    ran.push(e)
    return { result: 'ok' }
  })

  await $.prompt.submit({ text: 'Read me the plan.', wait: false, origin: { kind: 'composer' } })
  const result = await $.tool.call({ tool: SPEAK, text: 'Here it is.' })

  expect(sent).toEqual([[SPOKEN_NOTE]])
  expect(refusal(result)).toBeUndefined()
  expect(ran).toEqual([expect.objectContaining({ tool: SPEAK, text: 'Here it is.' })])
  expect(ran[0]).not.toHaveProperty('session')
  // The plugin's own call named the session, which ties Claude Code's
  // connection to it.
  expect(told[0]).toEqual({
    tool: 'turn',
    args: expect.objectContaining({ event: 'prompt', session: expect.objectContaining({}) }),
  })
})
