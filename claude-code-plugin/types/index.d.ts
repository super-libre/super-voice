// SPDX-License-Identifier: GPL-3.0-only
// The state the plugin's hooks module keeps, by name.

/** Whether Super Voice is speaking, as `voice status` last said. */
export type SuperVoiceIsSpeaking = boolean

/** Whether Claude is recording the user through the `listen` tool. */
export type SuperVoiceIsClaudeListening = boolean

/** Whether the reply being read answers a spoken prompt, so the microphone
 * opens after it, and stopping it starts the user talking. */
export type SuperVoiceListensAfterReply = boolean

/** Which reply block a 🔊 is reading aloud again, by its button's key; empty
 * when none is. */
export type SuperVoiceReadingAgain = string

/** Whether Claude's main turn is running, so sending stops it first. */
export type SuperVoiceIsClaudeWorking = boolean

/** Whether the user shows Talk above the prompt, as Super Voice last said:
 * the switch on the Assistants page. Off, the plugin draws nothing there. */
export type SuperVoiceShowsTalk = boolean

/** Where the Talk button is: waiting, recording, transcribing, counting down
 * to send, or waiting for Enter on an answer the user is editing. */
export type SuperVoiceTalk =
  | { phase: 'idle' }
  | { phase: 'recording' }
  | { phase: 'transcribing' }
  | { phase: 'countdown'; secondsLeft: number }
  | { phase: 'editing' }

declare module 'claude-code' {
  interface PluginState {
    'super-voice': {
      isSpeaking: SuperVoiceIsSpeaking
      isClaudeListening: SuperVoiceIsClaudeListening
      listensAfterReply: SuperVoiceListensAfterReply
      isClaudeWorking: SuperVoiceIsClaudeWorking
      readingAgain: SuperVoiceReadingAgain
      talk: SuperVoiceTalk
      showsTalk: SuperVoiceShowsTalk
    }
  }
}
