// SPDX-License-Identifier: GPL-3.0-only
//
// What the TUI part asks of the server part, which holds the connection to
// Super Voice: the two run in separate processes.

import { Rpc } from '@opencode/plugin/rpc'

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
    /** Stop the session's own speech, and end its recording. */
    stop: { input: session, output: { type: 'object', additionalProperties: false } },
    /** Whether the session's speech plays or it records, and whether its
     * turn under way was spoken. */
    status: {
      input: session,
      output: {
        type: 'object',
        properties: {
          speaking: { type: 'boolean' },
          listening: { type: 'boolean' },
          conversation: { type: 'boolean' },
        },
        required: ['speaking', 'listening', 'conversation'],
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
