// SPDX-License-Identifier: GPL-3.0-only
//
// The terms Talk sends with its `listen` call, so Super Voice hears the names
// of the conversation the user is answering. Super Voice adds them to the
// user's own context for that one take.

/** The most terms one take adds. */
export const MAX_TERMS = 50
/** The longest term worth sending: anything longer is not said aloud. */
const MAX_TERM_CHARS = 64

/** All-caps words that are words, not names. */
const NOT_ACRONYMS = new Set(['OK', 'TODO', 'NOTE'])

/**
 * The names and terms the user may say next, picked from `text`, the last
 * reply and prompt: what was quoted as code, file and folder names,
 * identifiers, acronyms and names. In the order they first appear, without
 * repeats, and at most {@link MAX_TERMS}.
 */
export function pickTerms(text: string): string[] {
  const terms: string[] = []
  const seen = new Set<string>()
  const add = (term: string) => {
    if (terms.length >= MAX_TERMS || term.length < 2 || term.length > MAX_TERM_CHARS) {
      return
    }
    const key = term.toLowerCase()
    if (!seen.has(key)) {
      seen.add(key)
      terms.push(term)
    }
  }
  // A code block is code to read, not words to say.
  const prose = text.replace(/```[\s\S]*?```/g, '\n')
  // What was quoted as code is named exactly, so it comes first.
  for (const [, span] of prose.matchAll(/`([^`\n]+)`/g)) {
    const quoted = span.trim()
    add(isPath(quoted) ? lastPart(quoted) : quoted)
  }
  for (const line of prose.replace(/`[^`\n]+`/g, ' ').split('\n')) {
    pickFromLine(line, add)
  }
  return terms
}

function pickFromLine(line: string, add: (term: string) => void) {
  // A run of capitalized words is one name: "Claude Code".
  let name: string[] = []
  let nameStartsSentence = false
  const endName = () => {
    if (name.length > 1 || (name.length === 1 && !nameStartsSentence)) {
      add(name.join(' '))
    }
    name = []
  }
  let startsSentence = true
  for (const raw of line.split(/\s+/)) {
    const word = bare(raw)
    if (!word) {
      continue
    }
    if (isCapitalized(word)) {
      if (name.length === 0) {
        nameStartsSentence = startsSentence
      }
      name.push(word)
    } else {
      endName()
    }
    if (isPath(word)) {
      add(lastPart(word))
    } else if (isIdentifier(word) || isAcronym(word)) {
      add(word)
    }
    // A name ends where its sentence or clause does.
    if (/[.!?:;,)]$/.test(raw.replace(/["'*_]+$/, ''))) {
      endName()
    }
    startsSentence = /[.!?:]["'*_)]*$/.test(raw)
  }
  endName()
}

/** `raw` without the punctuation and markup around it, or a possessive. */
function bare(raw: string): string {
  return raw
    .replace(/^[^\p{L}\p{N}~./_-]+/u, '')
    .replace(/[^\p{L}\p{N}/_-]+$/u, '')
    .replace(/['’]s$/u, '')
}

/** A file or folder: a path, or a name with an extension. A web address is
 * not said aloud. */
function isPath(word: string): boolean {
  if (word.includes('://')) {
    return false
  }
  if (/^~?\.?\/?[\w.-]+(\/[\w.-]+)+\/?$/.test(word)) {
    return true
  }
  // `register.tsx`, not "e.g" or "3.5".
  return /^[\w-]*[A-Za-z][\w-]*\.[A-Za-z][A-Za-z0-9]{0,4}$/.test(word) && !/^\w\.\w$/.test(word)
}

/** What a path is called: its last part. */
function lastPart(path: string): string {
  const parts = path.split('/').filter(Boolean)
  return parts[parts.length - 1] ?? path
}

/** camelCase, snake_case, or a kebab-case name with a number in it. */
function isIdentifier(word: string): boolean {
  return (
    /[a-z][A-Z]/.test(word) ||
    /^[A-Za-z]\w*_\w+$/.test(word) ||
    (/^[A-Za-z][\w]*(-[\w]+)+$/.test(word) && /\d/.test(word))
  )
}

function isAcronym(word: string): boolean {
  return /^[A-Z][A-Z0-9]{1,9}$/.test(word) && !NOT_ACRONYMS.has(word)
}

function isCapitalized(word: string): boolean {
  return /^\p{Lu}[\p{Ll}\p{N}]+$/u.test(word) && word !== 'I'
}
