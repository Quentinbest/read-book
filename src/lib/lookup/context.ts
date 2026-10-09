// LK5 (Reading Lens): the context a lookup sees around a selection. It is derived
// from the selection's DOM range, never found by string search, so a word that
// appears three times gets the sentence of the occurrence the reader selected.
//
// The chapter is one document: an L16 chunked chapter keeps its hidden blocks in
// the DOM, and each B8 Scroll-mode view holds a whole section, so the context can
// continue across chunks and views but never crosses into another chapter (EP3).
// Footnote markers and note bodies are left out; soft hyphens are removed.

import { extractText, offsetAt, type ExtractedText } from '../search/extract'
import { isNote, isNoteRef } from '../../reader/notes'

/** LK5 (prov.): the most characters a context holds, everything together. */
export const CONTEXT_MAX = 1200

export interface SelectionContext {
  /** The sentence before the selected one, or "". */
  before: string
  /** The sentence(s) holding the selection. */
  sentence: string
  /** The sentence after, or "". */
  after: string
  /** The paragraph holding the selection, cut to the cap around the sentence. */
  paragraph: string
  /** The chapter's label (its Contents entry), or "". */
  chapter: string
  /** Where the selection sits in `sentence` (LK3: it is underlined while pending). */
  selection: { start: number; end: number }
}

type Span = [number, number]

const SOFT_HYPHEN = /­/g
/** Abbreviations after which ICU breaks a sentence that has not ended. */
const ABBREVIATION =
  /(?:^|[\s(])(?:Mr|Mrs|Ms|Dr|Prof|Sr|Jr|St|vs|etc|cf|al|Fig|Figs|Eq|Eqs|No|Nos|Vol|Ch|Sec|pp|ca|approx|i\.e|e\.g|[A-Z])\.\s*$/

const segmenters = new Map<string, Intl.Segmenter>()
function segmenter(lang: string): Intl.Segmenter {
  let s = segmenters.get(lang)
  if (!s) {
    try {
      s = new Intl.Segmenter(lang || undefined, { granularity: 'sentence' })
    } catch {
      s = new Intl.Segmenter(undefined, { granularity: 'sentence' })
    }
    segmenters.set(lang, s)
  }
  return s
}

/** Sentence spans of `text` inside [from, to), whole sentences only, abbreviations rejoined. */
export function sentences(text: string, from: number, to: number, lang = ''): Span[] {
  const out: Span[] = []
  for (const seg of segmenter(lang).segment(text.slice(from, to))) {
    const a = from + seg.index
    const b = a + seg.segment.length
    const prev = out[out.length - 1]
    // Whitespace between blocks (indented markup) is not a sentence of its own.
    if (!seg.segment.trim()) {
      if (prev) prev[1] = b
      continue
    }
    // A break after an abbreviation or an initial, inside one line, is not a sentence end.
    if (
      prev &&
      !text.slice(prev[0], prev[1]).includes('\n') &&
      ABBREVIATION.test(text.slice(prev[0], prev[1]))
    )
      prev[1] = b
    else out.push([a, b])
  }
  return out
}

/** The span without the whitespace at its ends. */
function trim(text: string, [a, b]: Span): Span {
  while (a < b && /\s/.test(text[a])) a++
  while (b > a && /\s/.test(text[b - 1])) b--
  return [a, b]
}

const len = ([a, b]: Span) => b - a
const union = (x: Span, y: Span): Span => [Math.min(x[0], y[0]), Math.max(x[1], y[1])]

/**
 * LK5 on plain text: the context of [start, end) in a chapter's text, whose blocks
 * are separated by newlines (extractText). Everything together stays within `max`.
 */
export function contextOf(
  text: string,
  start: number,
  end: number,
  options: { lang?: string; chapter?: string; max?: number } = {},
): SelectionContext {
  const max = options.max ?? CONTEXT_MAX
  const lang = options.lang ?? ''
  // Segment a window around the selection only: a 1 MB chapter is not segmented whole.
  const reach = max + 400
  const from = Math.max(0, start - reach)
  const to = Math.min(text.length, end + reach)
  let spans = sentences(text, from, to, lang)
  // A window edge can cut a sentence: drop the partial ones.
  if (from > 0 && spans.length > 1) spans = spans.slice(1)
  if (to < text.length && spans.length > 1) spans = spans.slice(0, -1)
  const at = (offset: number) => {
    const i = spans.findIndex(([a, b]) => offset >= a && offset < b)
    return i < 0 ? (offset < (spans[0]?.[0] ?? 0) ? 0 : spans.length - 1) : i
  }
  const first = at(start)
  const last = at(Math.max(start, end - 1))
  let sentence = trim(text, [spans[first]?.[0] ?? start, spans[last]?.[1] ?? end])
  sentence = union(sentence, [start, end])
  if (len(sentence) > max) {
    // A sentence longer than the cap: keep the selection, centred.
    const selection: Span = [start, Math.min(end, start + max)]
    const room = max - len(selection)
    const a = Math.max(sentence[0], selection[0] - Math.floor(room / 2))
    const b = Math.min(sentence[1], a + max)
    sentence = [Math.max(sentence[0], b - max), b]
  }
  let used = sentence
  const neighbour = (i: number): Span | null => {
    const s = spans[i]
    if (!s) return null
    const t = trim(text, s)
    return len(t) && len(union(used, t)) <= max ? t : null
  }
  const before = neighbour(first - 1)
  if (before) used = union(used, before)
  const after = neighbour(last + 1)
  if (after) used = union(used, after)
  // The paragraph: the line holding the sentence, cut to what the cap leaves, around it.
  const lineStart = text.lastIndexOf('\n', sentence[0] - 1) + 1
  const lineEndAt = text.indexOf('\n', sentence[1])
  const line = trim(text, [lineStart, lineEndAt < 0 ? text.length : lineEndAt])
  let paragraph: Span = [Math.max(line[0], sentence[0]), Math.min(line[1], sentence[1])]
  for (let grew = true; grew;) {
    grew = false
    for (const s of spans) {
      const t = trim(text, s)
      if (t[0] < line[0] || t[1] > line[1]) continue
      const next = union(paragraph, t)
      if (next[0] === paragraph[0] && next[1] === paragraph[1]) continue
      // Only sentences next to what the paragraph holds, and only while everything fits.
      if (t[1] < paragraph[0] - 2 || t[0] > paragraph[1] + 2) continue
      if (len(union(used, next)) > max) continue
      paragraph = next
      grew = true
    }
  }
  const clean = (s: Span | null) => (s ? text.slice(s[0], s[1]).replace(SOFT_HYPHEN, '') : '')
  const sentenceText = text.slice(sentence[0], sentence[1])
  const shift = (offset: number) =>
    sentenceText.slice(0, Math.max(0, offset - sentence[0])).replace(SOFT_HYPHEN, '').length
  return {
    before: clean(before),
    sentence: clean(sentence),
    after: clean(after),
    paragraph: clean(paragraph),
    chapter: options.chapter ?? '',
    selection: { start: shift(start), end: shift(Math.min(end, sentence[1])) },
  }
}

/** The characters a context sends: what it holds without repeats (AC5). */
export function contextLength(c: SelectionContext): number {
  const inParagraph = (s: string) => !s || c.paragraph.includes(s)
  return (
    c.paragraph.length +
    (inParagraph(c.before) ? 0 : c.before.length) +
    (inParagraph(c.after) ? 0 : c.after.length) +
    (inParagraph(c.sentence) ? 0 : c.sentence.length)
  )
}

/** Footnote markers and note bodies are not part of the reading text around a selection. */
const notReadingText = (el: Element): boolean =>
  el.localName === 'aside' ? isNote(el) : isNoteRef(el)

const extracted = new WeakMap<Document, ExtractedText>()

/** The chapter's text without markers and notes, extracted once per document. */
function extraction(doc: Document): ExtractedText {
  let x = extracted.get(doc)
  if (!x) {
    x = extractText(doc.body ?? doc.documentElement, notReadingText)
    extracted.set(doc, x)
  }
  return x
}

/**
 * Spike H: a selection's chapter is read and the segmenter made in idle time, so
 * choosing a lookup finds them ready (≤ 10 ms per selection).
 */
export function warmContext(doc: Document, lang = ''): void {
  extraction(doc)
  segmenter(lang)
}

/** LK5: the context of a selection's range, in its own chapter's document. */
export function selectionContext(
  range: Range,
  options: { lang?: string; chapter?: string; max?: number } = {},
): SelectionContext | null {
  const doc = range.startContainer.ownerDocument
  if (!doc) return null
  const x = extraction(doc)
  const start = offsetAt(x, range.startContainer, range.startOffset)
  const end = offsetAt(x, range.endContainer, range.endOffset)
  if (start === null || end === null || end <= start) return null
  return contextOf(x.text, start, end, options)
}

/** LK9: sentences in a selection's text. */
export function sentenceCount(text: string, lang = ''): number {
  const t = text.trim()
  return t ? sentences(t, 0, t.length, lang).length : 0
}
