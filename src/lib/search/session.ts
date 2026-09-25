// A search over a book whose chapters are still arriving (F3, F8). The worker
// wraps this: chapters are indexed as the reader extracts them, and a running
// search scans each one when it arrives, in F3 order (current chapter first,
// then onward, wrapping), so results stream in and progress can be shown.

import {
  indexChapter,
  parseQuery,
  searchChapter,
  type ChapterText,
  type IndexedChapter,
  type Match,
  type ParsedQuery,
} from './search'

export type SearchEvent =
  | { type: 'chapter'; id: number; index: number; matches: Match[] }
  | { type: 'progress'; id: number; searched: number; total: number }
  | { type: 'done'; id: number; searched: number; total: number }

interface Active {
  id: number
  query: ParsedQuery
  /** Chapters in F3 order, and which of them have been scanned. */
  order: number[]
  next: number
  scanned: Set<number>
}

export class SearchSession {
  #chapters = new Map<number, IndexedChapter>()
  #active: Active | null = null

  get indexed(): number {
    return this.#chapters.size
  }

  /** Index chapters; a running search scans the ones it is waiting for. */
  add(chapters: ChapterText[]): SearchEvent[] {
    for (const c of chapters) this.#chapters.set(c.index, indexChapter(c))
    return this.#advance()
  }

  /**
   * Start a search (replacing any running one). `order` lists every searchable
   * chapter in F3 order; the ones not yet indexed are scanned when they arrive.
   */
  search(id: number, raw: string, order: number[]): SearchEvent[] {
    const query = parseQuery(raw)
    if (!query) {
      this.#active = null
      return [{ type: 'done', id, searched: 0, total: 0 }]
    }
    this.#active = { id, query, order, next: 0, scanned: new Set() }
    return this.#advance()
  }

  cancel() {
    this.#active = null
  }

  /** Scan every indexed chapter the search has not seen, keeping F3 order for results. */
  #advance(): SearchEvent[] {
    const a = this.#active
    if (!a) return []
    const events: SearchEvent[] = []
    // Results go out in order: stop at the first chapter that is not indexed yet.
    while (a.next < a.order.length) {
      const chapter = this.#chapters.get(a.order[a.next])
      if (!chapter) break
      const results = searchChapter(chapter, a.query)
      a.scanned.add(chapter.index)
      if (results.matches.length)
        events.push({ type: 'chapter', id: a.id, index: chapter.index, matches: results.matches })
      a.next++
    }
    const total = a.order.length
    const searched = a.scanned.size
    if (a.next >= total) {
      events.push({ type: 'done', id: a.id, searched, total })
      this.#active = null
    } else events.push({ type: 'progress', id: a.id, searched, total })
    return events
  }
}
