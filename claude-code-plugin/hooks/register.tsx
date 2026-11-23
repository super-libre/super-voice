// SPDX-License-Identifier: GPL-3.0-only
// The plugin's voice, a Claude Code hooks module. It works through Claude
// Code's own connection to Super Voice's MCP server, and names this session
// on every call, so what it speaks and records is this session's own.
// Claude speaks only after being spoken to: a reply to a spoken prompt is
// read aloud and the microphone opens after it, and Super Voice's tools are
// refused in a turn the user typed. One row above the prompt and one key, 0: `Talk` records the user and
// puts their words in the prompt box, sent after a short countdown unless they
// edit them; while speech plays, 0 stops it, and opens the microphone only in
// a spoken conversation. What `listen` hears goes through the box the same
// way. A 🔊 under each reply block reads it again.

import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, Timer } from 'claude-code'

import type { SuperVoiceTalk } from '../types'
import { talkArgs } from './terms'
import { type Answer, NO_SPEECH, NOT_SPOKEN_TO, PLUGIN_TOOL_NAMES, SPOKEN_NOTE, fieldsOf } from './voice'

const speaking = atom({ plugin: 'super-voice', key: 'isSpeaking' } as const, false)
const claudeListening = atom(
  { plugin: 'super-voice', key: 'isClaudeListening' } as const,
  false,
)
const opensMicAfter = atom(
  { plugin: 'super-voice', key: 'opensMicAfterReply' } as const,
  false,
)
const claudeWorking = atom(
  { plugin: 'super-voice', key: 'isClaudeWorking' } as const,
  false,
)
const readingAgain = atom({ plugin: 'super-voice', key: 'readingAgain' } as const, '')
const talk = atom(
  { plugin: 'super-voice', key: 'talk' } as const,
  { phase: 'idle' } as SuperVoiceTalk,
)
const showsTalk = atom({ plugin: 'super-voice', key: 'showsTalk' } as const, true)

/** This plugin's name, as `prompt.change` and `prompt.submit` name a plugin. */
const NAME = 'super-voice'
/** How often to ask the daemon while speech may be playing. */
const POLL_MS = 500
/**
 * How long to wait for speech to start after something asked for it: the
 * daemon may load its model first.
 */
const START_WAIT_MS = 10_000
/** How long the user's words wait in the prompt box before they are sent. */
const SEND_AFTER_MS = 5_000
/** How often the countdown checks the box and redraws. */
const COUNTDOWN_TICK_MS = 500
/** Super Voice's MCP server, by its key in this plugin's `.mcp.json`. */
const SERVER_KEY = 'voice'
/** How often the session checks in: until Super Voice has heard it started,
 * which waits on Claude Code connecting; so Super Voice keeps listing it; and
 * to learn whether the user shows Talk, so the switch on the Assistants page
 * takes effect soon after it moves. */
const CHECK_IN_MS = 5_000
/** The tools this plugin calls itself, kept from the model. */
const PLUGIN_TOOLS: ReadonlySet<string> = new Set(
  PLUGIN_TOOL_NAMES.map(tool => `mcp__plugin_super-voice_voice__${tool}`),
)
/** The Super Voice tools that speak. `listen` asks its question aloud. */
const LISTEN_TOOL = 'mcp__plugin_super-voice_voice__listen'
const SPEAKING_TOOLS: ReadonlySet<string> = new Set([
  'mcp__plugin_super-voice_voice__speak',
  LISTEN_TOOL,
])

// Watching for speech.
let speechTimer: Timer | undefined
let isChecking = false
/** What the row last showed. Unknown after a reload, so the first check writes
 * whatever it finds. */
let shownSpeaking: boolean | undefined
/** Until when to keep asking while no speech has been seen. Zero once speech
 * was seen, so the first silence after it ends the watch. */
let waitUntil = 0
/** Whether this watch has heard speech, so its end is speech that finished. */
let sawSpeech = false

// The Talk button.
let phase: SuperVoiceTalk['phase'] = 'idle'
let countdownTimer: Timer | undefined
let sendAt = 0
/** The prompt box as the fill left it. Anything else means the user is
 * editing, which stops the countdown. */
let filled: { text: string; cursor: number } | undefined
/** Claude's `listen` call waiting on the user's answer, which they can check
 * in the prompt box first: what to hand it once it is sent. */
let pendingAnswer: ((text: string | undefined) => void) | undefined
/** Claude's running main turn, which sending the user's words stops first,
 * as their Ctrl+Enter does: a plugin's prompt otherwise waits for it to end. */
let runningTurnId: string | undefined

// Listening after a spoken reply.
/** The words Talk put in the box since the last prompt was sent. */
let talked: string | undefined
/** The prompt now being answered was spoken, so its reply is read aloud, and
 * Claude may speak and listen. */
let isTurnSpoken = false
/** The main agent's last prompt and reply, which Talk picks the names and
 * terms of the conversation from. */
let lastPrompt = ''
let lastReply = ''
/** The reply being read answers a spoken prompt: when it has been read to the
 * end, or the user stops it, open the microphone. Typing calls it off. */
let armed = false


// This session, as Super Voice's tools are told it.
/** The folder the session works in; empty when it could not be read. */
let folder: string | undefined
/** The session id Super Voice was last told started. */
let openedId: string | undefined
let checkInTimer: Timer | undefined
/** A check-in is under way: one waiting on Claude Code's connection can
 * outlast the timer's beat. */
let isCheckingIn = false

/** Super Voice's MCP server as Claude Code names it, once connected. */
let server = 'plugin:super-voice:voice'

/** Whether `origin`, the one that changed or sent the prompt, is this
 * plugin. */
function isOwnOrigin(origin: { kind: string }): boolean {
  return origin.kind === 'plugin' && (origin as { name?: string }).name === NAME
}

/** This session as Super Voice's tools take it: `id` when given, else the
 * session's own. */
async function sessionOf($: EngineInterface, id?: string) {
  if (folder === undefined) {
    folder = await $.session.cwd().catch(() => '')
  }
  return { id: id ?? (await $.session.id()), ...(folder ? { folder } : {}) }
}

/** Call one of Super Voice's tools for this session, or for session `id`,
 * through Claude Code's own connection: its answer, an error with no text
 * when the call failed. */
async function call(
  $: EngineInterface,
  tool: string,
  args: Record<string, unknown> = {},
  id?: string,
): Promise<Answer> {
  try {
    const result = await $.mcp.call(server, tool, { ...args, session: await sessionOf($, id) })
    const text = result.content.map(block => (block.type === 'text' ? block.text : '')).join('')
    return { text, isError: result.isError }
  } catch {
    return { text: '', isError: true }
  }
}

/** Tell Super Voice of the prompt the user sent, and whether it was spoken:
 * `isTalk` for Talk's own prompt. Otherwise as Super Voice says: spoken when
 * it still holds `heard`, the words Talk put in the box, though the user may
 * have touched them up, or what they dictated into the box with Super
 * Voice's own shortcut. Typed, when Super Voice can't say. */
async function promptWasSpoken(
  $: EngineInterface,
  text: string,
  isTalk: boolean,
  heard?: string,
): Promise<boolean> {
  const answer = await call($, 'turn', {
    event: 'prompt',
    text,
    ...(isTalk ? { spoken: true } : {}),
    ...(heard ? { heard } : {}),
  })
  return isTalk || fieldsOf(answer).spoken === true
}

type VoiceStatus = { isSpeaking: boolean; isListening: boolean }

/** Whether this session's own speech plays, and whether it records; nothing
 * when Super Voice can't say. */
async function voiceStatus($: EngineInterface): Promise<VoiceStatus | undefined> {
  const answer = await call($, 'status')
  if (answer.isError) {
    return undefined
  }
  const { speaking: isSpeaking, listening: isListening, talk: isShown } = fieldsOf(answer)
  if (typeof isShown === 'boolean' && isShown !== (await read($, showsTalk))) {
    await update($, showsTalk, () => isShown)
  }
  return { isSpeaking: isSpeaking === true, isListening: isListening === true }
}

/** Tell Super Voice this session is open, and check in while it is, which
 * also learns whether the user shows Talk. A new session id, as after
 * `/clear`, ends the one before. */
async function checkIn($: EngineInterface) {
  if (isCheckingIn) {
    return
  }
  isCheckingIn = true
  try {
    await checkInOnce($)
  } finally {
    isCheckingIn = false
  }
}

async function checkInOnce($: EngineInterface) {
  const id = await $.session.id()
  if (openedId === id) {
    await voiceStatus($)
    return
  }
  // Claude Code may still be connecting to Super Voice as the session starts.
  const voice = await $.mcp.connect(SERVER_KEY).catch(() => undefined)
  if (!voice?.isConnected) {
    return
  }
  server = voice.server
  if (openedId !== undefined) {
    await call($, 'session', { event: 'end' }, openedId)
    // A new conversation, as after `/clear`: the old one's terms are not its.
    lastPrompt = ''
    lastReply = ''
  }
  const started = await call($, 'session', { event: 'start' })
  // Not yet: the next check-in tries again.
  if (!started.isError) {
    openedId = id
  }
}

async function setArmed($: EngineInterface, isArmed: boolean) {
  if (armed !== isArmed) {
    armed = isArmed
    await update($, opensMicAfter, () => isArmed)
  }
}

async function showSpeaking($: EngineInterface, isOn: boolean) {
  if (shownSpeaking !== isOn) {
    shownSpeaking = isOn
    await update($, speaking, () => isOn)
  }
}

async function checkSpeech($: EngineInterface) {
  if (isChecking) {
    return
  }
  isChecking = true
  try {
    const isOn = (await voiceStatus($))?.isSpeaking ?? false
    await showSpeaking($, isOn)
    if (isOn) {
      waitUntil = 0
      sawSpeech = true
    } else if ((await $.clock.now()) >= waitUntil) {
      speechTimer?.cancel()
      speechTimer = undefined
      const isReplyRead = sawSpeech && armed
      sawSpeech = false
      await setArmed($, false)
      // Whatever a 🔊 was reading has ended, or never started.
      await update($, readingAgain, () => '')
      // With Talk not shown, nothing records without a row to say so.
      if (isReplyRead && (await read($, showsTalk))) {
        void startTalk($)
      }
    }
  } finally {
    isChecking = false
  }
}

/** Ask the daemon until the speech that was just asked for has played. */
async function watchSpeech($: EngineInterface) {
  waitUntil = (await $.clock.now()) + START_WAIT_MS
  sawSpeech = false
  speechTimer ??= $.clock.every(POLL_MS, () => void checkSpeech($))
}

async function stopSpeaking($: EngineInterface) {
  await setArmed($, false)
  await showSpeaking($, false)
  await update($, readingAgain, () => '')
  await call($, 'stop')
}

/** 0 while speech plays: it always stops. Cutting off Claude's reply to a
 * spoken prompt is the user's turn to talk, so the microphone opens; any other
 * speech, like a 🔊 or another session's, just stops. Cutting off a question
 * of Claude's `listen` needs nothing more: it records once the question
 * stops. */
async function pressStopSpeaking($: EngineInterface) {
  const isConversation = armed
  // The stop comes first, whatever else is going on.
  await stopSpeaking($)
  if (isConversation && !(await read($, claudeListening)) && phase === 'idle') {
    await startTalk($)
  }
}

/** Tell Super Voice Claude answered the turn with `answer`. Super Voice reads
 * the reply to a spoken prompt aloud; once it has been read, the microphone
 * opens. */
async function replied($: EngineInterface, answer: string) {
  const turn = await call($, 'turn', { event: 'reply', text: answer })
  if (turn.isError) {
    $.ui.toast(turn.text || 'Could not read the reply aloud.')
    return
  }
  if (fieldsOf(turn).read === true) {
    await setArmed($, true)
    await watchSpeech($)
  }
}

/** Read one of Claude's reply blocks aloud again, or stop it when it is the
 * one being read: the button is a toggle. */
async function readAgain($: EngineInterface, key: string, text: string) {
  // Hearing a reply again is not a turn to answer.
  await setArmed($, false)
  if ((await read($, readingAgain)) === key) {
    await stopSpeaking($)
    return
  }
  await update($, readingAgain, () => key)
  // The user asked for it: a typed turn does not refuse it.
  const spoken = await call($, 'speak', { text, asked: true })
  if (spoken.isError) {
    await update($, readingAgain, () => '')
    $.ui.toast(spoken.text || 'Could not read it aloud.')
    return
  }
  await watchSpeech($)
}

async function setTalk($: EngineInterface, next: SuperVoiceTalk) {
  phase = next.phase
  await update($, talk, () => next)
}

/** Record the user, then put their words in the prompt box. Returns once the
 * recording has started; the rest runs on. */
async function startTalk($: EngineInterface) {
  if (phase !== 'idle') {
    return
  }
  if ((await voiceStatus($))?.isListening) {
    $.ui.toast('Super Voice is already recording.')
    return
  }
  await setTalk($, { phase: 'recording' })
  // Claude's voice would be recorded too.
  await stopSpeaking($)
  void recordAndFill($)
}

async function recordAndFill($: EngineInterface) {
  let words = ''
  const heard = await call($, 'listen', talkArgs(lastPrompt, lastReply))
  if (heard.isError) {
    $.ui.toast(heard.text || 'Could not record.')
  } else {
    words = heard.text.trim()
  }
  if (words === NO_SPEECH) {
    $.ui.toast('No speech heard.')
    words = ''
  }
  if (!words) {
    await setTalk($, { phase: 'idle' })
    return
  }
  await fillAndCount($, words)
}

/** Stop the recording of Claude's `listen` call: it answers with what it
 * heard. */
async function stopClaudesListening($: EngineInterface) {
  await setTalk($, { phase: 'transcribing' })
  await call($, 'stop')
}

/** Put what `listen` heard in the prompt box, and resolve with it once it is
 * sent: by the countdown as it stands, or by the user's Enter as they edited
 * it. Nothing when the turn is stopped first; the words stay in the box. */
function reviewAnswer(
  $: EngineInterface,
  words: string,
  signal: AbortSignal,
): Promise<string | undefined> {
  return new Promise(resolve => {
    pendingAnswer = resolve
    signal.addEventListener('abort', () => void finishAnswer($, undefined))
    void fillAndCount($, words)
  })
}

async function finishAnswer($: EngineInterface, text: string | undefined) {
  const resolve = pendingAnswer
  if (resolve === undefined) {
    return
  }
  pendingAnswer = undefined
  await stopCountdown($)
  if (text !== undefined) {
    await $.prompt.fill({ text: '' })
  }
  resolve(text)
}

async function stopListening($: EngineInterface) {
  if (phase !== 'recording') {
    // The row outlived its recording: nothing here is waiting on one.
    await setTalk($, { phase: 'idle' })
    return
  }
  // Shown before the stop, so a quick transcript is not drawn over by it.
  await setTalk($, { phase: 'transcribing' })
  // The recording ends and `recordAndFill` carries on with what was heard.
  await call($, 'stop')
}

/** Put the words in the box after any draft, and send them unless the user
 * starts editing first. */
async function fillAndCount($: EngineInterface, words: string) {
  const draft = (await $.prompt.read()).text.trimEnd()
  const box = await $.prompt.fill({ text: draft ? `${draft} ${words}` : words })
  if (!box.isFilled) {
    if (pendingAnswer !== undefined) {
      // No box to check it in: Claude gets the words as heard.
      await finishAnswer($, words)
      return
    }
    $.ui.toast('Could not fill the prompt box.')
    await setTalk($, { phase: 'idle' })
    return
  }
  if (pendingAnswer === undefined) {
    talked = talked ? `${talked} ${words}` : words
  }
  filled = { text: box.text, cursor: box.cursor }
  sendAt = (await $.clock.now()) + SEND_AFTER_MS
  await setTalk($, { phase: 'countdown', secondsLeft: Math.ceil(SEND_AFTER_MS / 1000) })
  countdownTimer = $.clock.every(COUNTDOWN_TICK_MS, () => void tick($))
}

async function tick($: EngineInterface) {
  if (phase !== 'countdown' || filled === undefined) {
    return
  }
  // A click that moves the cursor fires no edit, so look at the box.
  const box = await $.prompt.read()
  if (box.text !== filled.text || box.cursor !== filled.cursor) {
    await stopCountdown($)
    return
  }
  const left = sendAt - (await $.clock.now())
  if (left > 0) {
    const secondsLeft = Math.ceil(left / 1000)
    const shownTalk = await read($, talk)
    if (shownTalk.phase !== 'countdown' || shownTalk.secondsLeft !== secondsLeft) {
      await setTalk($, { phase: 'countdown', secondsLeft })
    }
    return
  }
  if (pendingAnswer !== undefined) {
    await finishAnswer($, box.text)
    return
  }
  await stopCountdown($)
  await $.prompt.fill({ text: '' })
  if (runningTurnId !== undefined) {
    // Said while Claude works: it should hear it now, not when it is done.
    await $.turn.abort({ turnId: runningTurnId }).catch(() => undefined)
  }
  // The words are the user's own, so Claude reads them bare. Hooks and
  // permission checks still see this mod as the sender.
  await $.prompt.submit({ text: box.text, asUser: true }).catch(() => {
    $.ui.toast('Could not send.')
  })
}

async function stopCountdown($: EngineInterface) {
  countdownTimer?.cancel()
  countdownTimer = undefined
  filled = undefined
  // An answer the user is editing still waits for their Enter.
  await setTalk($, pendingAnswer === undefined ? { phase: 'idle' } : { phase: 'editing' })
}

export const register: Register = on => {
  // The session's state outlives a reload, but the recording, countdown and
  // speech watch of the copy before do not. Start the rows over to match.
  on('session.start', async ($, e, next) => {
    await update($, talk, () => ({ phase: 'idle' }))
    await update($, speaking, () => false)
    await update($, claudeListening, () => false)
    await update($, opensMicAfter, () => false)
    await update($, claudeWorking, () => false)
    await update($, readingAgain, () => '')
    // A reload starts the session's check-ins over too. The Talk switch is
    // kept as last seen until Super Voice says.
    openedId = undefined
    checkInTimer?.cancel()
    checkInTimer = $.clock.every(CHECK_IN_MS, () => void checkIn($))
    void checkIn($)
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    checkInTimer?.cancel()
    checkInTimer = undefined
    await call($, 'session', { event: 'end' }, e.sessionId)
    openedId = undefined
    return next(e)
  })

  // The tools this plugin calls itself are not the model's.
  on('tool.offer', async ($, e, next) =>
    PLUGIN_TOOLS.has(e.tool) ? { isOffered: false } : next(e),
  )

  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      runningTurnId = e.turnId
      await update($, claudeWorking, () => true)
    }
    return next(e)
  })

  // A reply to a spoken prompt is read aloud, and the microphone opens after
  // it. A reply to a typed one stays silent.
  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined) {
      runningTurnId = undefined
      await update($, claudeWorking, () => false)
      if (e.answer.trim()) {
        lastReply = e.answer
      }
      if (e.isAborted) {
        // A spoken prompt that stopped this turn is still to be answered.
        await setArmed($, false)
      } else {
        isTurnSpoken = false
        await replied($, e.answer)
      }
    }
    return next(e)
  })

  on('tool.call', async ($, e, next) => {
    if (!SPEAKING_TOOLS.has(e.tool)) {
      return next(e)
    }
    // This plugin's own calls, as Talk and reading a reply make them, name
    // their session; Claude's never do, as no tool declares it. Claude Code
    // runs a call this plugin makes inside one of its own hooks through this
    // hook too, and reading a reply comes after its turn is over.
    if ('session' in e) {
      return next(e)
    }
    // Claude speaks only after being spoken to.
    if (!isTurnSpoken) {
      return { deny: NOT_SPOKEN_TO }
    }
    // Named as this session, as the plugin's own calls are.
    const named = { ...e, session: await sessionOf($) }
    await watchSpeech($)
    if (e.tool !== LISTEN_TOOL) {
      return next(named)
    }
    // Claude has the microphone: no Talk button until it is done.
    await update($, claudeListening, () => true)
    try {
      return await next(named)
    } finally {
      await update($, claudeListening, () => false)
    }
  })

  // What `listen` heard goes through the prompt box, as Talk's words do:
  // Claude gets it once it is sent, as the user left it.
  on('tool.run', { tool: LISTEN_TOOL }, async ($, e, next) => {
    const heard = await next(e)
    if (phase === 'transcribing') {
      await setTalk($, { phase: 'idle' })
    }
    if (heard.deny !== undefined || heard.isError) {
      return heard
    }
    const words = (heard.text ?? '').trim()
    if (!words || words === NO_SPEECH) {
      // Nothing was said: the user stopped the recording or stayed quiet. The
      // question is dismissed, so Claude's turn ends here, as the user's Esc
      // would end it, with nothing for it to answer.
      if (runningTurnId !== undefined) {
        await $.turn.abort({ turnId: runningTurnId }).catch(() => undefined)
      }
      return heard
    }
    if (phase !== 'idle') {
      return heard
    }
    let answer: { text: string | undefined } | undefined
    void reviewAnswer($, words, next.signal).then(text => {
      answer = { text }
    })
    // A hook's own time is bounded, and waiting on a promise counts against
    // it; time inside a `$` call does not. So wait for the user through short
    // sleeps on the host.
    while (answer === undefined) {
      await $.process.run(['sleep', '0.25'], { timeoutMs: 5000 }).catch(() => undefined)
    }
    return answer.text === undefined ? heard : { result: answer.text }
  })

  // A key in the box, a cursor move included: the user is editing, or about
  // to type instead of talk.
  on('prompt.edit', async ($, e, next) => {
    await setArmed($, false)
    if (phase === 'countdown') {
      await stopCountdown($)
    }
    return next(e)
  })

  // A recalled history entry, a clear, another plugin's fill.
  on('prompt.change', async ($, e, next) => {
    if (phase === 'countdown' && !isOwnOrigin(e.origin)) {
      await stopCountdown($)
    }
    return next(e)
  })

  on('prompt.submit', async ($, e, next) => {
    // A new prompt silences what this session was reading.
    if (await read($, speaking)) {
      void stopSpeaking($)
    }
    if (e.origin.kind === 'composer' && pendingAnswer !== undefined) {
      // Enter on an answer to Claude's question: it goes to the waiting
      // `listen` call, not into the queue as a prompt.
      await finishAnswer($, e.text)
      return { drop: "Sent as the answer to Claude's question." }
    }
    const isOwn = isOwnOrigin(e.origin)
    // Another plugin's prompt is neither the user's nor Talk's.
    if (e.origin.kind !== 'composer' && !isOwn) {
      return next(e)
    }
    // Enter before the countdown ended: the prompt is the user's, and still
    // spoken, so the talk goes on after the reply. Sent while Claude works,
    // it is delivered into that turn, whose reply answers it.
    if (phase === 'countdown') {
      await stopCountdown($)
    }
    // Talk's own prompt is spoken; the user's when it holds Talk's words, or
    // ones they dictated with Super Voice's shortcut.
    const isSpoken = await promptWasSpoken($, e.text, isOwn, talked)
    lastPrompt = e.text
    isTurnSpoken = isSpoken
    talked = undefined
    await setArmed($, false)
    return next(isSpoken ? { ...e, context: [...(e.context ?? []), SPOKEN_NOTE] } : e)
  })

  // A speaker under each block of Claude's replies reads that block aloud
  // again. On the terminal it takes a click, in the fullscreen layout.
  on('ui.render', { component: 'AssistantMessage' }, async ($, e, next) => {
    const row = await next(e)
    const { text, isSummary } = e.props
    if (isSummary || !text.trim()) {
      return row
    }
    const { Box, Button } = $.ui.resolve(e)
    // Blocks of one message share its id; their text tells them apart.
    const key = `read-again-${e.requestId}-${text.length}`
    const isReading = (await read($, readingAgain)) === key
    return (
      <Box flexDirection="column">
        {row}
        <Button
          key={key}
          // Both glyphs are emoji two columns wide: a narrower stop left only
          // half of what the terminal draws to click.
          label={isReading ? '\u{1F507}' : '\u{1F50A}'}
          plain
          dimColor={!isReading}
          onPress={() => void readAgain($, key, text)}
        />
      </Box>
    )
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    // The user turned Talk off on the Assistants page: nothing at all here,
    // and the 0 key types a 0.
    if (e.props.hasSurvey || !(await read($, showsTalk))) {
      return next(e)
    }
    const { Box, Button, Text } = $.ui.resolve(e)
    const state = await read($, talk)
    if (state.phase === 'recording') {
      return (
        <Box>
          <Text dimColor>Listening. </Text>
          <Button
            key="stop-listening"
            label="Stop listening"
            hotkey="0"
            plain
            onPress={() => void stopListening($)}
          />
        </Box>
      )
    }
    if (state.phase === 'transcribing') {
      return <Text dimColor>Transcribing.</Text>
    }
    if (state.phase === 'countdown') {
      const stopsClaude = await read($, claudeWorking)
      return (
        <Text dimColor>
          Sending in {state.secondsLeft} s{stopsClaude ? ', and stopping Claude' : ''}. Press
          Enter to send now, or edit to stop.
        </Text>
      )
    }
    if (state.phase === 'editing') {
      return <Text dimColor>Press Enter to answer Claude.</Text>
    }
    const isListening = await read($, claudeListening)
    if (await read($, speaking)) {
      const talksAfter = isListening || (await read($, opensMicAfter))
      return (
        <Button
          key="stop-speaking"
          label={talksAfter ? 'Stop and talk' : 'Stop speaking'}
          hotkey="0"
          plain
          onPress={() => void pressStopSpeaking($)}
        />
      )
    }
    if (isListening) {
      return (
        <Box>
          <Text dimColor>Listening. </Text>
          <Button
            key="stop-claudes-listening"
            label="Stop listening"
            hotkey="0"
            plain
            onPress={() => void stopClaudesListening($)}
          />
        </Box>
      )
    }
    return (
      <Button
        key="talk"
        label="Talk"
        hotkey="0"
        plain
        onPress={() => void startTalk($)}
      />
    )
  })
}
