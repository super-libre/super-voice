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

const ABOVE_PROMPT = { plugin: 'super-voice', surface: 'terminal', component: 'AbovePrompt' } as const
const TASK = 'klk5xtabl'
const MOVED =
  `MCP tool "plugin:super-voice:voice/listen" is still running after 120s. It was moved to the ` +
  `background as task ${TASK} and keeps running; you'll receive a notification with the result ` +
  `when it completes. You can keep working in the meantime.`

test("Talk's recording that Claude Code moved to the background stops, and its end is kept from Claude", async ($, on) => {
  on('session.id', () => ({ value: 'session-1' }))
  on('session.cwd', () => ({ value: '/home/user/project' }))
  const told: string[] = []
  on('mcp.call', (_, e) => {
    told.push(e.tool)
    const text = e.tool === 'listen' ? MOVED : e.tool === 'status' ? '{}' : 'Stopped.'
    return { value: { content: [{ type: 'text', text }] } }
  })
  const filled: string[] = []
  on('prompt.fill', (_, e) => {
    filled.push(e.text)
    return { isFilled: true, text: e.text, cursor: e.text.length }
  })
  const sent: string[] = []
  on('prompt.submit', (_, e) => {
    sent.push(e.text)
    return { text: e.text, origin: e.origin }
  })

  const ui = await $.ui.mount({
    ...ABOVE_PROMPT,
    props: { hasSurvey: false, isCollapsed: false, isWorking: false, maxRows: 10 },
  } as Parameters<typeof $.ui.mount>[0])
  await ui.press({ key: 'talk' })

  // The recording is stopped once Claude Code has moved it, and nothing goes
  // in the box.
  expect(told.slice(-2)).toEqual(['listen', 'stop'])
  expect(filled).toEqual([])

  const other = await $.prompt.submit({
    text: '<task-notification>\n<task-id>b0ther</task-id>\n<status>completed</status>\n</task-notification>',
    wait: false,
    origin: { kind: 'task-notification' },
  })
  const ended = await $.prompt.submit({
    text: `<task-notification>\n<task-id>${TASK}</task-id>\n<status>completed</status>\n<result>Hello</result>\n</task-notification>`,
    wait: false,
    origin: { kind: 'task-notification' },
  })

  expect(other).not.toHaveProperty('drop')
  expect(ended).toHaveProperty('drop')
  expect(sent).toHaveLength(1)
  await ui.unmount()
})
