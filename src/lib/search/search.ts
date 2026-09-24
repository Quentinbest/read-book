// In-book search over a per-chapter text index (plan F2–F4, F8).

import { hasCJK, normalize, type NormalizedText } from './normalize'

export interface ChapterText {
  /** Spine index of the chapter. */
  index: number
  /** Plain text of the chapter, as extracted from its DOM. */
  text: string
}

export interface IndexedChapter {
  index: number
  original: string
  folded: NormalizedText
  literal: NormalizedText
}

export interface Match {
  /** Offsets into the chapter's original text. */
  start: number
  end: number
  snippet: Snippet
}

export interface Snippet {
  before: string
  match: string
  after: string
}

export interface ChapterResults {
  index: number
  matches: Match[]
}

export interface ParsedQuery {
  needle: string
  exact: boolean
}

export const MIN_QUERY_LENGTH = 2
export const MIN_QUERY_LENGTH_CJK = 1
export const SNIPPET_LENGTH = 90

export function indexChapter({ index, text }: ChapterText): IndexedChapter {
  return {
    index,
    original: text,
    folded: normalize(text, 'folded'),
    literal: normalize(text, 'literal'),
  }
}

const QUOTED = /^\s*["“”]([^"“”]+)["“”]\s*$/

/**
 * Parse what the reader typed. A query wrapped in quotes matches that exact
 * phrase, case and diacritics included; anything else ignores case and
 * diacritics (F2). Returns null when the query is below the minimum length.
 */
export function parseQuery(raw: string): ParsedQuery | null {
  const quoted = QUOTED.exec(raw)
  const exact = quoted !== null
  const needle = normalize(quoted ? quoted[1] : raw, exact ? 'literal' : 'folded').text.trim()
  const min = hasCJK(needle) ? MIN_QUERY_LENGTH_CJK : MIN_QUERY_LENGTH
  if ([...needle].length < min) return null
  return { needle, exact }
}

/** Chapters in search order: the current chapter first, then onward, wrapping (F3). */
export function searchOrder<T extends { index: number }>(chapters: T[], current: number): T[] {
  const sorted = [...chapters].sort((a, b) => a.index - b.index)
  const at = sorted.findIndex((c) => c.index >= current)
  return at <= 0 ? sorted : [...sorted.slice(at), ...sorted.slice(0, at)]
}

export function searchChapter(chapter: IndexedChapter, query: ParsedQuery): ChapterResults {
  const { text, map } = query.exact ? chapter.literal : chapter.folded
  const matches: Match[] = []
  let from = 0
  for (;;) {
    const at = text.indexOf(query.needle, from)
    if (at < 0) break
    const start = map[at]
    // End where the next original cluster starts, so trailing combining marks
    // and whole ligatures are included. map[text.length] is the original length.
    const last = map[at + query.needle.length - 1]
    let next = at + query.needle.length
    while (next < text.length && map[next] === last) next++
    const end = map[next]
    matches.push({ start, end, snippet: snippet(chapter.original, start, end) })
    from = at + query.needle.length
  }
  return { index: chapter.index, matches }
}

/** Search chapters in F3 order, yielding each chapter's results as soon as it is scanned. */
export function* searchBook(
  chapters: IndexedChapter[],
  query: ParsedQuery,
  current: number,
): Generator<ChapterResults> {
  for (const chapter of searchOrder(chapters, current)) {
    const results = searchChapter(chapter, query)
    if (results.matches.length) yield results
  }
}

const SPACE = /\s/

/**
 * About SNIPPET_LENGTH characters around the match, cut at word boundaries
 * where the text has them (F4). CJK text without spaces is cut at characters.
 */
export function snippet(text: string, start: number, end: number): Snippet {
  const room = Math.max(0, SNIPPET_LENGTH - (end - start))
  let from = Math.max(0, start - Math.floor(room / 2))
  let to = Math.min(text.length, end + Math.ceil(room / 2))
  // Give unused room on one side to the other.
  if (from === 0) to = Math.min(text.length, end + room - start)
  if (to === text.length) from = Math.max(0, start - (room - (text.length - end)))
  if (from > 0) {
    const space = text.slice(from, start).search(SPACE)
    if (space >= 0 && space < start - from) from += space + 1
  }
  if (to < text.length) {
    const tail = text.slice(end, to)
    const lastSpace = Math.max(tail.lastIndexOf(' '), tail.lastIndexOf('\n'))
    if (lastSpace > 0) to = end + lastSpace
  }
  const clean = (s: string) => s.replace(/\s+/g, ' ')
  return {
    before: (from > 0 ? '…' : '') + clean(text.slice(from, start)).trimStart(),
    match: clean(text.slice(start, end)),
    after: clean(text.slice(end, to)).trimEnd() + (to < text.length ? '…' : ''),
  }
}
