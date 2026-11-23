// SPDX-License-Identifier: GPL-3.0-only
//
// What the TUI part asks of the server part, which holds the connection to
// Super Voice: the two run in separate processes.

import { Rpc } from '@opencode/plugin/rpc'

/** A call about one session, by its id. */
export type SessionInput = { sessionID: string }
/** `read`'s input: the session, and how far back from its last reply. */
export type ReadInput = SessionInput & { back: number }
/** What Talk heard, or why it heard nothing. */
export type Heard = { text?: string; error?: string }
/** What a call that answers nothing on success came to: why it failed. */
export type Outcome = { error?: string }
/** What this session's speech is doing, whether its turn under way was
 * spoken, and whether the user shows Talk. */
export type Status = { speaking: boolean; conversation: boolean; talk: boolean }

const session = {
  type: 'object',
  properties: { sessionID: { type: 'string' } },
  required: ['sessionID'],
  additionalProperties: false,
} as const

export const Voice = Rpc.define({
  id: 'super-voice',
  methods: {
    /** Record the user until they stop talking: what they said, or why not. */
    talk: {
      input: session,
      output: {
        type: 'object',
        properties: { text: { type: 'string' }, error: { type: 'string' } },
        additionalProperties: false,
      },
    },
    /** Read one of the session's replies aloud again, counted back from the
     * last one, 0: nothing, or why not. */
    read: {
      input: {
        type: 'object',
        properties: {
          sessionID: { type: 'string' },
          back: { type: 'integer', minimum: 0, maximum: 9 },
        },
        required: ['sessionID', 'back'],
        additionalProperties: false,
      },
      output: {
        type: 'object',
        properties: { error: { type: 'string' } },
        additionalProperties: false,
      },
    },
    /** Stop the session's own speech, and end its recording. */
    stop: { input: session, output: { type: 'object', additionalProperties: false } },
    /** Whether the session's speech plays, whether its turn under way was
     * spoken, and whether the user shows Talk. */
    status: {
      input: session,
      output: {
        type: 'object',
        properties: {
          speaking: { type: 'boolean' },
          conversation: { type: 'boolean' },
          talk: { type: 'boolean' },
        },
        required: ['speaking', 'conversation', 'talk'],
        additionalProperties: false,
      },
    },
  },
  events: {
    /** A spoken prompt's reply is being read aloud: the microphone opens
     * once it has been. */
    replied: {
      schema: {
        type: 'object',
        properties: { sessionID: { type: 'string' } },
        required: ['sessionID'],
        additionalProperties: false,
      },
    },
  },
})
