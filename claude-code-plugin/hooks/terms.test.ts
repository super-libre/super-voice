// SPDX-License-Identifier: GPL-3.0-only
import { expect, test } from 'claude-code/testing'

import { MAX_TERMS, pickTerms, talkArgs } from './terms'

test('a reply gives the names and terms the user may say back', () => {
  const reply = [
    'I moved the check into `handle_record_internal` and renamed',
    '~/super-libre/super-voice/claude-code-plugin/hooks/register.tsx to keep',
    'Claude Code and the Antigravity CLI apart. The voxtral-mini-3b model takes',
    'a context, and so does the OpenAI backend; see https://example.com/docs.',
    '',
    '```rust',
    'let ignored = codeBlockOnly();',
    '```',
    'Then run just ci. It passed.',
  ].join('\n')
  expect(pickTerms(reply)).toEqual([
    'handle_record_internal',
    'register.tsx',
    'Claude Code',
    'Antigravity',
    'CLI',
    'voxtral-mini-3b',
    'OpenAI',
  ])
})

test('a sentence that starts with a capital is not a name', () => {
  expect(pickTerms('Moved it. Done for now, OK?')).toEqual([])
})

test('a term comes once, and the list stops at its limit', () => {
  expect(pickTerms('`agy` and agyCli, then `AGY` and agyCli')).toEqual(['agy', 'agyCli'])
  const many = Array.from({ length: MAX_TERMS + 10 }, (_, n) => `fieldName${n}`).join(' ')
  expect(pickTerms(many).length).toBe(MAX_TERMS)
})

test("Talk's listen is the user's asking, with the conversation's terms when it has any", () => {
  expect(talkArgs('', '')).toEqual({ asked: true })
  expect(talkArgs('Rename agyCli.', 'Renamed it to agyHooks.')).toEqual({
    asked: true,
    context: { vocabulary: ['agyHooks', 'agyCli'] },
  })
})
