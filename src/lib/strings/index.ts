// The UI's messages in the current language (B10, L-1). Features import `t` from
// here. Each page sets its language once, before it imports anything that reads
// `t` (src/app/locale.ts); a change applies at restart (L-5).

import { en } from './en'
import { pseudoLocalize } from './pseudo'
import type { Locale, Messages } from './types'

export type { Locale, Messages } from './types'

/** The catalogue of each language Linen ships. English is the source and the fallback. */
const CATALOGUES: Partial<Record<Locale, () => Messages>> = {
  en: () => en,
  ...(import.meta.env.DEV ? { 'en-XA': () => pseudoLocalize(en) } : {}),
}

/** The languages this build offers, English first. */
export const AVAILABLE = Object.keys(CATALOGUES) as Locale[]

/** Each language named in its own script, as Settings lists them (L-4). */
export const LANGUAGE_NAMES: Record<Locale, string> = {
  en: 'English',
  'zh-Hans': '简体中文',
  'zh-Hant': '繁體中文',
  ja: '日本語',
  es: 'Español',
  'en-XA': 'Pseudo (en-XA)',
}

/** Keys a catalogue lacked, filled from English (dev builds warn; tests read it). */
export const missingKeys: string[] = []

export let locale: Locale = 'en'
export let t: Messages = en

/** Use `l` for this page. An unknown language means English. */
export function setLocale(l: Locale): void {
  const make = CATALOGUES[l]
  locale = make ? l : 'en'
  missingKeys.length = 0
  t = make && l !== 'en' ? (withFallback(make(), en, '') as Messages) : en
}

/** A gap in a catalogue shows English, never a key. */
function withFallback(m: unknown, base: unknown, path: string): unknown {
  if (m === undefined || typeof m !== typeof base) {
    missingKeys.push(path)
    if (import.meta.env.DEV) console.warn(`[strings] ${locale} lacks ${path}`)
    return base
  }
  if (typeof base !== 'object' || base === null) return m
  return Object.fromEntries(
    Object.entries(base).map(([k, v]) => [
      k,
      withFallback((m as Record<string, unknown>)[k], v, path ? `${path}.${k}` : k),
    ]),
  )
}
