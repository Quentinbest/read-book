// Which UI language to use (L-2, L-3, L-4): the `language` setting if it names an
// available language, otherwise the first of the reader's preferred languages that
// Linen has, otherwise English.

import type { Locale } from './types'

/** The `language` setting's key, and its default value, which follows macOS. */
export const LANGUAGE_SETTING = 'language'
export const SYSTEM = 'system'

/**
 * A preferred language as Linen keys it: Chinese by script (zh-TW, zh-HK and zh-MO
 * are Traditional; L-2), everything else by language alone (es-419 → es; L-3).
 */
export function languageKey(tag: string): string {
  const [lang = '', ...rest] = tag.toLowerCase().replace(/_/g, '-').split('-')
  if (lang !== 'zh') return lang
  const script = rest.find((p) => p.length === 4)
  const region = rest.find((p) => p.length === 2 || /^\d{3}$/.test(p))
  const traditional = script ? script === 'hant' : ['tw', 'hk', 'mo'].includes(region ?? '')
  return traditional ? 'zh-Hant' : 'zh-Hans'
}

/** The first preferred language Linen has, or English. */
export function negotiate(preferred: readonly string[], available: readonly Locale[]): Locale {
  for (const tag of preferred) {
    const exact = available.find((l) => l.toLowerCase() === tag.toLowerCase())
    if (exact) return exact
    const match = available.find((l) => l === languageKey(tag))
    if (match) return match
  }
  return 'en'
}

/** L-4: the setting wins when it names an available language; anything else follows macOS. */
export function resolveLocale(
  setting: string | null,
  preferred: readonly string[],
  available: readonly Locale[],
): Locale {
  return available.find((l) => l === setting) ?? negotiate(preferred, available)
}
