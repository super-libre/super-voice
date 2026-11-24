// SPDX-License-Identifier: GPL-3.0-only
//
// What Super Voice's plugins for Claude Code and OpenCode say and read alike.
// OpenCode's plugin links this file, as it does terms.ts.

/** What `listen` answers when it heard nothing. */
export const NO_SPEECH = '(no speech detected)'

/** What the model reads beside a spoken prompt, so it answers for listening
 * and works as it always does. */
export const SPOKEN_NOTE =
  'The user said this aloud, through Super Voice. Do the work as you normally would. ' +
  'Only your final reply is read aloud to them, and the microphone opens after it so ' +
  'they can answer by voice: keep that reply short and plain to listen to, end it with ' +
  'any question you have, and do not call `listen` to hear their answer.'

/** Why the model's call to a Super Voice tool is refused in a turn the user
 * typed. */
export const NOT_SPOKEN_TO =
  'The user typed this prompt, so answer in writing. Super Voice speaks and listens only ' +
  'while the user is talking to you by voice, which they start with Talk.'

/** A tool's answer: its text, and whether it failed. */
export type Answer = { text: string; isError: boolean }

/** The fields of a tool's JSON answer: none when the call failed, or its text
 * is not a JSON object. */
export function fieldsOf(answer: Answer): Record<string, unknown> {
  if (answer.isError) {
    return {}
  }
  try {
    const value: unknown = JSON.parse(answer.text)
    return typeof value === 'object' && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {}
  } catch {
    return {}
  }
}

/** The Super Voice tools the model calls. */
export const MODEL_TOOL_NAMES = ['speak', 'listen'] as const

/** The Super Voice tools a plugin calls itself, kept from the model. */
export const PLUGIN_TOOL_NAMES = ['stop', 'status', 'announce', 'session', 'turn'] as const
