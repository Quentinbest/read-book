// Release 1.1 reading settings (plan §1.3 “1.1”; P§9; C5). PROVISIONAL until the
// owner approves them (docs/pending-approvals.md).
//
// - Font family: the book's own, Literata, a sans, or a dyslexia-friendly face.
// - Page width: the measure of L1 at its narrow end, its default, or its wide end.
// - Publisher styles: full, balanced (the MVP's behaviour) or off; a book can be
//   simplified on its own (“Simplify styles”, P§22), which is Off for that book.

import { MEASURE_CH, MEASURE_MAX_CH, MEASURE_MIN_CH } from './layout'

export type FontChoice = 'book' | 'literata' | 'sans' | 'dyslexic'
export type PageWidth = 'narrow' | 'normal' | 'wide'
export type PublisherStyles = 'full' | 'balanced' | 'off'

export const FONT_CHOICES: FontChoice[] = ['book', 'literata', 'sans', 'dyslexic']
export const PAGE_WIDTHS: PageWidth[] = ['narrow', 'normal', 'wide']
export const PUBLISHER_STYLES: PublisherStyles[] = ['full', 'balanced', 'off']

/** L1: the measure in ch for each width; the ends are L1's clamp. */
export const PAGE_WIDTH_CH: Record<PageWidth, number> = {
  narrow: MEASURE_MIN_CH,
  normal: MEASURE_CH,
  wide: MEASURE_MAX_CH,
}

/** The family each choice forces over the book's own fonts. */
export const FONT_STACKS: Record<Exclude<FontChoice, 'book'>, string> = {
  literata: 'Literata, Georgia, serif',
  sans: '-apple-system, "Helvetica Neue", Helvetica, sans-serif',
  dyslexic: 'OpenDyslexic, -apple-system, sans-serif',
}

const pick =
  <T extends string>(all: readonly T[], fallback: T) =>
  (v: string | null | undefined): T =>
    all.includes(v as T) ? (v as T) : fallback

export const parseFont = pick(FONT_CHOICES, 'book')
export const parsePageWidth = pick(PAGE_WIDTHS, 'normal')
export const parsePublisherStyles = pick(PUBLISHER_STYLES, 'balanced')

/** The settings keys (all books) and the per-book key for Simplify styles. */
export const SETTING = {
  font: 'fontFamily',
  width: 'pageWidth',
  publisher: 'publisherStyles',
} as const
export const simplifyKey = (bookId: string) => `simplifyStyles:${bookId}`

/** C5: a simplified book shows no publisher styles, whatever the global setting. */
export const effectivePublisherStyles = (
  global: PublisherStyles,
  simplified: boolean,
): PublisherStyles => (simplified ? 'off' : global)
