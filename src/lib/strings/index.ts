// The UI's messages in the current language (B10, L-1). Features import `t` from
// here. Each page sets its language once, before it imports anything that reads
// `t` (src/app/locale.ts); a change applies at restart (L-5).

import { setCurrent } from './current'
import { en } from './en'
import { pseudoLocalize } from './pseudo'
import type { Locale, Messages } from './types'

export type { Locale, Messages } from './types'
export { locale } from './current'

/**
 * The catalogue of each language Linen ships. English is the source and the
 * fallback. Registering a language here means adding it to src-tauri/Info.plist
 * too (docs/i18n/README.md, “Ship”).
 */
const SHIPPED_CATALOGUES: Partial<Record<Locale, () => Messages>> = {
  en: () => en,
}

/**
 * Development builds also offer the pseudo-locale and every draft catalogue in
 * this folder, so a translation can be read in the app before it ships.
 */
const DEV_CATALOGUES: Partial<Record<Locale, () => Messages>> = import.meta.env.DEV
  ? {
      ...Object.fromEntries(
        Object.entries(
          import.meta.glob<Record<string, Messages>>('./{zh-Hans,zh-Hant,ja,es}.ts', {
            eager: true,
          }),
        ).map(([path, mod]) => [path.slice(2, -3), () => Object.values(mod)[0]]),
      ),
      'en-XA': () => pseudoLocalize(en),
    }
  : {}

const CATALOGUES = { ...SHIPPED_CATALOGUES, ...DEV_CATALOGUES, ...SHIPPED_CATALOGUES }

/** The languages that ship (Info.plist lists the same). */
export const SHIPPED = Object.keys(SHIPPED_CATALOGUES) as Locale[]

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

export let t: Messages = en

/** Use `l` for this page. An unknown language means English. */
export function setLocale(l: Locale): void {
  const make = CATALOGUES[l]
  setCurrent(make ? l : 'en')
  missingKeys.length = 0
  t = make && l !== 'en' ? (withFallback(make(), en, '', l) as Messages) : en
}

/** A gap in a catalogue shows English, never a key. */
function withFallback(m: unknown, base: unknown, path: string, l: Locale): unknown {
  if (m === undefined || typeof m !== typeof base) {
    missingKeys.push(path)
    if (import.meta.env.DEV) console.warn(`[strings] ${l} lacks ${path}`)
    return base
  }
  if (typeof base !== 'object' || base === null) return m
  return Object.fromEntries(
    Object.entries(base).map(([k, v]) => [
      k,
      withFallback((m as Record<string, unknown>)[k], v, path ? `${path}.${k}` : k, l),
    ]),
  )
}
