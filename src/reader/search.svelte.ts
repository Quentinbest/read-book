// Search state for the reader (F1–F4, F6): the query (run 150 ms after typing
// stops), results streaming in by chapter, progress while indexing, and the
// active result.

import type { Match } from '../lib/search/search'
import type { BookSearch } from './bookSearch'

/** F2: search 150 ms after typing stops. */
export const SEARCH_DEBOUNCE_MS = 150
/** B6: results shown per chapter before “Show all N”. */
export const RESULTS_PER_CHAPTER = 100

export interface Hit {
  index: number
  /** Position of the match in its chapter's list. */
  n: number
  match: Match
}

export class SearchState {
  query = $state('')
  /** Replaced whole, never mutated: raw, so thousands of matches are not made deeply reactive. */
  groups: { index: number; matches: Match[] }[] = $state.raw([])
  searched = $state(0)
  total = $state(0)
  running = $state(false)
  /** A search has finished (or not started) for the current query. */
  settled = $state(true)
  active = $state<{ index: number; n: number } | null>(null)
  #cancel: (() => void) | null = null
  #timer = 0

  constructor(
    private book: () => BookSearch | null,
    private currentSection: () => number,
    private changed: () => void,
  ) {}

  /** For the budget check (see BookSearch.timings). */
  get timings() {
    return this.book()?.timings
  }

  get count(): number {
    return this.groups.reduce((n, g) => n + g.matches.length, 0)
  }

  get hits(): Hit[] {
    return this.groups.flatMap((g) => g.matches.map((match, n) => ({ index: g.index, n, match })))
  }

  /** 1-based position of the active result, or 0. */
  get position(): number {
    const a = this.active
    if (!a) return 0
    return this.hits.findIndex((h) => h.index === a.index && h.n === a.n) + 1
  }

  setQuery(q: string) {
    this.query = q
    this.settled = false
    clearTimeout(this.#timer)
    this.#timer = window.setTimeout(() => this.run(), SEARCH_DEBOUNCE_MS)
  }

  /** Run now (↵ before the debounce has fired, or a pre-filled query). */
  run() {
    clearTimeout(this.#timer)
    this.#cancel?.()
    this.groups = []
    this.active = null
    this.searched = 0
    const book = this.book()
    if (!book || !this.query.trim()) {
      this.running = false
      this.settled = true
      this.changed()
      return
    }
    this.total = book.total
    this.running = true
    this.settled = false
    this.#cancel = book.search(this.query, this.currentSection(), (events) => {
      // A batch lands in one update: new chapters' results, then progress or done.
      const found = events.flatMap((e) =>
        e.type === 'chapter' ? [{ index: e.index, matches: e.matches }] : [],
      )
      if (found.length) this.groups = [...this.groups, ...found]
      const last = events.findLast((e) => e.type === 'progress' || e.type === 'done')
      if (last && (last.type === 'progress' || last.type === 'done')) {
        this.searched = last.searched
        if (last.type === 'done') {
          this.running = false
          this.settled = true
        }
      }
      this.changed()
    })
  }

  /** The next or previous result from the active one (F6); wraps. */
  step(dir: 1 | -1): Hit | null {
    const hits = this.hits
    if (!hits.length) return null
    const at = this.position - 1
    const i = at < 0 ? (dir === 1 ? 0 : hits.length - 1) : (at + dir + hits.length) % hits.length
    return hits[i]
  }

  stop() {
    clearTimeout(this.#timer)
    this.#cancel?.()
    this.#cancel = null
    this.running = false
  }
}
