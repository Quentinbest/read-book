// The page's UI language, kept apart from the catalogues so they can use the
// formatting helpers (format.ts) without an import cycle. Set by setLocale (index.ts).

import type { Locale } from './types'

export let locale: Locale = 'en'

export function setCurrent(l: Locale): void {
  locale = l
}
