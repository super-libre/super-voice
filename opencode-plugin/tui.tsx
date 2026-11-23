// SPDX-License-Identifier: GPL-3.0-only
//
// Super Voice's OpenCode plugin, the TUI part: one row above the prompt and
// one key, 0, while the prompt is empty; in a prompt with text, 0 is typed.
// 0 is Talk: it records the user and sends what they said as a spoken
// prompt. While Super Voice speaks, 0 stops it, and in a spoken conversation
// starts Talk. While Talk records, 0 ends the recording, which is still sent. Once a spoken prompt's reply has been
// read aloud, Talk starts again by itself, unless the user has typed.
//
// With Talk turned off on Super Voice's Assistants page, there is no row and
// no key: 0 types a 0, and Talk never starts by itself.
//
// OpenCode has no place under a reply for a button, so /read reads a reply
// aloud again instead, Talk shown or not.

import { Plugin } from '@opencode/plugin/tui'
import { Show, createSignal } from 'solid-js'

import { Voice } from './rpc'
import { NO_SPEECH } from './voice'

/** How often the row asks Super Voice what this session's speech is doing
 * while something goes on: speech, Talk, a spoken conversation, or a reply
 * about to be read. */
const POLL_MS = 1000
/** How often it asks otherwise, for speech started elsewhere and for the
 * Talk switch: seldom enough that a few OpenCode windows left open stay well
 * within what Super Voice lets one key ask. */
const QUIET_POLL_MS = 5000
/** How long to wait for a reply's reading to start before giving up on it. */
const START_WAIT_MS = 10_000
/** What `/read` takes. */
const READ_USAGE = '/read reads the last reply aloud; /read 1 the one before it, up to 9. /read again stops it.'

type Phase = 'idle' | 'listening' | 'sending'

export default Plugin.define({
  id: 'super-voice.tui',
  setup(context) {
    const voice = context.client.rpc(Voice)
    const [phase, setPhase] = createSignal<Phase>('idle')
    const [speaking, setSpeaking] = createSignal(false)
    const [conversation, setConversation] = createSignal(false)
    /** Whether the user shows Talk, as Super Voice last said. */
    const [shown, setShown] = createSignal(true)
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
      // A 0 typed into a prompt is the user's, whatever plays or records.
      if (!sessionID || !shown() || !promptIsEmpty()) {
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
      if (phase() !== 'idle') {
        return false
      }
      void talk(sessionID)
    }

    /** `/read` and `/read 3`: read the last reply aloud again, or the one
     * that many back. Plain `/read` stops what this session says. */
    const readAgain = async (input?: string) => {
      const sessionID = currentSession()
      if (!sessionID) {
        return
      }
      const given = (input ?? '').trim()
      if (!given && speaking()) {
        setSpeaking(false)
        await voice.stop({ sessionID }).catch(() => {})
        return
      }
      if (given && !/^[0-9]$/.test(given)) {
        context.ui.toast.show({ message: READ_USAGE, variant: 'info' })
        return
      }
      // Hearing a reply again is not a turn to answer.
      armed = undefined
      const answer = await voice
        .read({ sessionID, back: Number(given || '0') })
        .catch(() => ({ error: 'Super Voice is not reachable.' }))
      const { error } = answer as { error?: string }
      if (error) {
        context.ui.toast.show({ message: error, variant: 'error' })
      } else {
        setSpeaking(true)
        pollIn(POLL_MS)
      }
    }

    context.keymap.layer(() => ({
      commands: [{ id: 'super-voice.talk', title: 'Talk', bind: '0', run: press }],
    }))
    // Slash commands and the command palette offer only what every mode
    // reaches, as OpenCode's own /btw is.
    context.keymap.layer(() => ({
      mode: 'global',
      commands: [
        {
          id: 'super-voice.read',
          title: 'Read a reply aloud',
          description: READ_USAGE,
          group: 'Super Voice',
          palette: true,
          slash: { name: 'read', arguments: true },
          run: readAgain,
        },
      ],
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
      render: () => (
        <Show when={shown()}>
          <text fg={context.theme.text.muted}>{label()}</text>
        </Show>
      ),
    })

    const stopReplied = voice.events.on('replied', event => {
      const sessionID = (event.data as { sessionID: string }).sessionID
      // Nothing records without the row to say so.
      if (sessionID === currentSession() && shown()) {
        armed = { sessionID, since: Date.now(), heard: false }
        pollIn(0)
      }
    })

    // What this session's speech is doing, for the row, and to listen once a
    // reply has been read: asked often while something goes on.
    const isBusy = () => speaking() || conversation() || phase() !== 'idle' || armed !== undefined
    let pollTimer: ReturnType<typeof setTimeout> | undefined
    let isClosed = false
    const pollIn = (ms: number) => {
      clearTimeout(pollTimer)
      if (!isClosed) {
        pollTimer = setTimeout(async () => {
          await check()
          pollIn(isBusy() ? POLL_MS : QUIET_POLL_MS)
        }, ms)
      }
    }
    const check = async () => {
      const sessionID = currentSession()
      if (!sessionID) {
        setSpeaking(false)
        return
      }
      await voice
        .status({ sessionID })
        .then(status => {
          const {
            speaking: isSpeaking,
            conversation: isConversation,
            talk: isShown,
          } = status as {
            speaking: boolean
            conversation: boolean
            talk: boolean
          }
          setSpeaking(isSpeaking)
          setConversation(isConversation)
          setShown(isShown)
          if (!isShown) {
            armed = undefined
          }
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
    }
    pollIn(0)

    return () => {
      isClosed = true
      clearTimeout(pollTimer)
      stopReplied()
      row()
    }
  },
})
