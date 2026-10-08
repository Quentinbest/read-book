// The UI's messages in the current language (B10, L-1). Features import `t` from
// here. Each page sets its language once, before it imports anything that reads
// `t` (src/app/locale.ts); a change applies at restart (L-5).

import { withPhraseBreaks } from './breaks'
import { setCurrent } from './current'
import { en } from './en'
import { pseudoLocalize } from './pseudo'
import type { Locale, Messages } from './types'

export type { Locale, Messages } from './types'
export { locale } from './current'

/**
 * The languages Linen ships, each loaded when a page uses it: English is built in,
 * and the others stay out of the first paint (§6.4). Registering a language here
 * means adding it to src-tauri/Info.plist too (docs/i18n/README.md, “Ship”).
 */
const SHIPPED_LOADERS: Partial<Record<Locale, () => Promise<Messages>>> = {
  en: async () => en,
  // Signed off by native reviewers, 2026-10-08 (L-6, docs/decisions.md).
  'zh-Hans': () => import('./zh-Hans').then((m) => m.zhHans),
  'zh-Hant': () => import('./zh-Hant').then((m) => m.zhHant),
  ja: () => import('./ja').then((m) => m.ja),
  es: () => import('./es').then((m) => m.es),
}

/**
 * Catalogues ready to use. English always; development builds also have every
 * catalogue in this folder, drafts included, and the pseudo-locale, so a
 * translation can be read in the app before it ships.
 */
const loaded = new Map<Locale, Messages>([
  ['en', en],
  ...(import.meta.env.DEV
    ? [
        ...Object.entries(
          import.meta.glob<Record<string, Messages>>('./{zh-Hans,zh-Hant,ja,es}.ts', {
            eager: true,
          }),
        ).map(([path, mod]) => [path.slice(2, -3) as Locale, Object.values(mod)[0]] as const),
        ['en-XA', pseudoLocalize(en)] as const,
      ]
    : []),
])

/** The languages that ship (Info.plist lists the same). */
export const SHIPPED = Object.keys(SHIPPED_LOADERS) as Locale[]

/** The languages this build offers, English first. */
export const AVAILABLE = [...new Set([...SHIPPED, ...loaded.keys()])]

/** Fetch `l`'s catalogue, once; call before setLocale(l). An unknown language does nothing. */
export async function loadLocale(l: Locale): Promise<void> {
  const load = SHIPPED_LOADERS[l]
  if (load && !loaded.has(l)) loaded.set(l, await load())
}

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

/** Use `l` for this page (loaded with loadLocale). An unknown language means English. */
export function setLocale(l: Locale): void {
  const m = loaded.get(l)
  setCurrent(m ? l : 'en')
  missingKeys.length = 0
  t = m && l !== 'en' ? prepare(withFallback(m, en, '', l) as Messages, l) : en
}

/** Chinese and Japanese get break opportunities between phrases (breaks.ts). */
const prepare = (m: Messages, l: Locale): Messages => (/^(zh|ja)/.test(l) ? withPhraseBreaks(m) : m)

/**
 * The e2e harness only (src/spikes/i18n.ts): use `m` as language `l`, for drafts
 * and the pseudo-locale, which release builds leave out.
 */
export function adoptMessages(l: Locale, m: Messages): void {
  setCurrent(l)
  missingKeys.length = 0
  t = prepare(withFallback(m, en, '', l) as Messages, l)
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
