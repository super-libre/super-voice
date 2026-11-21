// SPDX-License-Identifier: GPL-3.0-only
//
// Super Voice's OpenCode plugin, the TUI part: one row above the prompt and
// one key, 0. While the prompt is empty, 0 is Talk: it records the user and
// sends what they said as a spoken prompt. While Super Voice speaks, 0 stops
// it, and in a spoken conversation starts Talk. While Talk records, 0 ends
// the recording, which is still sent. Once a spoken prompt's reply has been
// read aloud, Talk starts again by itself, unless the user has typed.

import { Plugin } from '@opencode/plugin/tui'
import { createSignal } from 'solid-js'

import { Voice } from './rpc'

/** What `listen` says when the user said nothing. */
const NO_SPEECH = '(no speech detected)'
/** How often the row asks Super Voice what this session's speech is doing. */
const POLL_MS = 1000
/** How long to wait for a reply's reading to start before giving up on it. */
const START_WAIT_MS = 10_000

type Phase = 'idle' | 'listening' | 'sending'

export default Plugin.define({
  id: 'super-voice.tui',
  setup(context) {
    const voice = context.client.rpc(Voice)
    const [phase, setPhase] = createSignal<Phase>('idle')
    const [speaking, setSpeaking] = createSignal(false)
    const [conversation, setConversation] = createSignal(false)
    /** The session whose reply is being read, to listen once it has been. */
    let armed: { sessionID: string; since: number; heard: boolean } | undefined

    const currentSession = (): string | undefined => {
      const route = context.ui.router.current() as { type?: string; sessionID?: string }
      return route.type === 'session' ? route.sessionID : undefined
    }
    const promptIsEmpty = () => !(context.renderer.currentFocusedEditor?.plainText ?? '').trim()

    const talk = async (sessionID: string) => {
      if (phase() !== 'idle') {
        return
      }
      armed = undefined
      setPhase('listening')
      const heard = await voice.talk({ sessionID }).catch(() => ({ error: 'Super Voice is not reachable.' }))
      const { text, error } = heard as { text?: string; error?: string }
      const words = (text ?? '').trim()
      if (error || !words || words === NO_SPEECH) {
        setPhase('idle')
        context.ui.toast.show({ message: error || 'No speech heard.', variant: error ? 'error' : 'info' })
        return
      }
      setPhase('sending')
      await context.client.session
        .prompt({ sessionID, text: words, metadata: { superVoice: { spoken: true } } })
        .catch(() => context.ui.toast.show({ message: 'Could not send what you said.', variant: 'error' }))
      setPhase('idle')
    }

    const press = () => {
      const sessionID = currentSession()
      if (!sessionID) {
        return false
      }
      if (phase() === 'listening') {
        void voice.stop({ sessionID })
        return
      }
      if (speaking()) {
        armed = undefined
        void voice.stop({ sessionID }).then(() => {
          if (conversation()) {
            void talk(sessionID)
          }
        })
        return
      }
      // A 0 typed into a prompt is the user's.
      if (!promptIsEmpty() || phase() !== 'idle') {
        return false
      }
      void talk(sessionID)
    }

    context.keymap.layer(() => ({
      commands: [{ id: 'super-voice.talk', title: 'Talk', bind: '0', run: press }],
    }))

    const label = () => {
      if (phase() === 'listening') {
        return 'Listening. 0: Stop listening'
      }
      if (phase() === 'sending') {
        return 'Sending what you said.'
      }
      if (speaking()) {
        return conversation() ? '0: Stop and talk' : '0: Stop speaking'
      }
      return '0: Talk'
    }

    const row = context.ui.slot({
      append: 'session.composer.top',
      render: () => <text fg={context.theme.text.muted}>{label()}</text>,
    })

    const stopReplied = voice.events.on('replied', event => {
      const sessionID = (event.data as { sessionID: string }).sessionID
      if (sessionID === currentSession()) {
        armed = { sessionID, since: Date.now(), heard: false }
      }
    })

    // What this session's speech is doing, for the row, and to listen once a
    // reply has been read.
    const poll = setInterval(() => {
      const sessionID = currentSession()
      if (!sessionID) {
        setSpeaking(false)
        return
      }
      void voice
        .status({ sessionID })
        .then(status => {
          const { speaking: isSpeaking, conversation: isConversation } = status as {
            speaking: boolean
            conversation: boolean
          }
          setSpeaking(isSpeaking)
          setConversation(isConversation)
          if (!armed || armed.sessionID !== sessionID) {
            return
          }
          if (!promptIsEmpty()) {
            // Typing calls it off.
            armed = undefined
          } else if (isSpeaking) {
            armed.heard = true
          } else if (armed.heard || Date.now() - armed.since > START_WAIT_MS) {
            armed = undefined
            void talk(sessionID)
          }
        })
        .catch(() => setSpeaking(false))
    }, POLL_MS)

    return () => {
      clearInterval(poll)
      stopReplied()
      row()
    }
  },
})
