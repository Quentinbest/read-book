// Stage 4 (docs/i18n-plan.md): the open book's language (its OPF metadata) for the
// book text the UI shows — Contents, search snippets, quotes, footnotes. Marked
// with it, WebKit draws Han characters in the book's forms (Chinese and Japanese
// differ) and VoiceOver reads it in the book's voice, whatever the UI language.

import { getContext, setContext } from 'svelte'

const KEY = Symbol('bookLang')

/** Called once by the Reader, which owns the book. */
export const setBookLang = (lang: string | null) => setContext(KEY, lang ?? undefined)

/** The book's language for a `lang` attribute, or undefined when the book gives none. */
export const bookLang = (): string | undefined => getContext(KEY)
