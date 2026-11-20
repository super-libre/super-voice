---
name: talk
description: Talk with the user by voice through Super Voice, asking and answering out loud until they say they are done. Use when the user asks to talk, to have a voice conversation, or to work hands-free.
---

# Talk by voice

The user wants to talk instead of type. Keep going in this one turn until they end it.

1. Hear the first request: call `listen` with the question "What would you like to do?"
2. Do the work as you would for a typed request.
3. Answer by calling `listen` again, with your answer as its `question`. This says the answer and then hears the reply, so the conversation goes on.
4. Repeat steps 2 and 3.

## What to say

1. One to three short sentences.
2. Say what you did or found, not how. Never read out code, file paths, commands or lists.
3. When you need a decision, end with the question.
4. When you need approval for something risky, ask for it out loud, and only go ahead on a clear yes.

## When to stop

1. The user says they are done ("that's all", "stop", "goodbye"): end the turn with a short written summary of what you did.
2. `listen` returns "(no speech detected)": ask once more. If it happens again, end the turn the same way.
3. A tool fails: say so in one sentence, and end the turn with the error written out.
