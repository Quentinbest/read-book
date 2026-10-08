// Stage 4 (docs/i18n-plan.md): line breaks between phrases in Chinese and Japanese.
// Their UIs set `word-break: keep-all` (src/app/base.css), so a line does not break
// inside a word; WebKit then breaks only at spaces. A zero-width space after
// full-width punctuation gives it the breaks between phrases (after 、 。 ， and so on).

import type { Messages } from './types'

const AFTER_PUNCTUATION = /([、。，：；！？」』）】〕〉》])(?=\S)/gu

/** `s` with a break opportunity after each full-width punctuation mark inside it. */
export const phraseBreaks = (s: string) => s.replace(AFTER_PUNCTUATION, '$1​')

function mapValue(v: unknown): unknown {
  if (typeof v === 'string') return phraseBreaks(v)
  if (typeof v === 'function')
    return (...args: unknown[]) => phraseBreaks((v as (...a: unknown[]) => string)(...args))
  if (v && typeof v === 'object')
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, mapValue(x)]))
  return v
}

/** A Chinese or Japanese catalogue with break opportunities between its phrases. */
export const withPhraseBreaks = (m: Messages): Messages => mapValue(m) as Messages
