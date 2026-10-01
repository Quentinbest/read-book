// A book's search index (F3, F8). Chapter text is extracted on the main thread
// (DOMParser is not available in workers), current chapter first, yielding
// between chapters; the worker normalises it and scans it. Extracted text is
// saved per book, so the next search of the same file starts at once.

import type { Book } from 'foliate-js/view.js'
import { ipc } from '../app/ipc'
import { extractText } from '../lib/search/extract'
import { searchOrder, type SearchOptions } from '../lib/search/search'
import type { SearchRequest } from '../lib/search/search.worker'
import type { SearchEvent } from '../lib/search/session'

/** Chapters saved to the core per write. */
const SAVE_BATCH = 20

export class BookSearch {
  #worker: Worker
  #sections: number[]
  #extracted = new Set<number>()
  #building: Promise<void> | null = null
  #handler: ((events: SearchEvent[]) => void) | null = null
  #nextId = 1
  #closed = false
  /** For the budget check: ms from a search's start to reading the saved index, first results, done. */
  timings = { saved: 0, first: 0, done: 0, painted: 0 }
  #started = 0

  constructor(
    private bookId: string,
    private book: Book,
  ) {
    this.#worker = new Worker(new URL('../lib/search/search.worker.ts', import.meta.url), {
      type: 'module',
    })
    this.#worker.onmessage = (e: MessageEvent<SearchEvent[]>) => this.#handler?.(e.data)
    this.#sections = book.sections.flatMap((s, i) => (s.linear === 'no' ? [] : [i]))
  }

  /** Chapters a search covers. */
  get total(): number {
    return this.#sections.length
  }

  /**
   * Search, streaming batches of events to `onEvents` (chapter results in F3
   * order, progress, done). Returns a function that cancels the search.
   */
  search(
    query: string,
    current: number,
    onEvents: (events: SearchEvent[]) => void,
    options: SearchOptions = {},
  ): () => void {
    const id = this.#nextId++
    this.#started = performance.now()
    this.timings = { saved: 0, first: 0, done: 0, painted: 0 }
    this.#handler = (events) => {
      const mine = events.filter((e) => e.id === id)
      if (!mine.length) return
      const t = Math.round(performance.now() - this.#started)
      this.timings.first ||= t
      if (mine.some((e) => e.type === 'done')) {
        this.timings.done = t
        // The first frame after the results are shown.
        requestAnimationFrame(
          () => (this.timings.painted = Math.round(performance.now() - this.#started)),
        )
      }
      onEvents(mine)
    }
    void this.#build(current)
    const order = searchOrder(
      this.#sections.map((index) => ({ index })),
      current,
    ).map((x) => x.index)
    this.#post({ type: 'search', id, query, order, options })
    return () => {
      if (this.#closed) return
      this.#post({ type: 'cancel' })
      this.#handler = null
    }
  }

  close() {
    this.#closed = true
    this.#worker.terminate()
  }

  #post(msg: SearchRequest) {
    if (!this.#closed) this.#worker.postMessage(msg)
  }

  /** F8: saved text first, then extract what is missing, current chapter onward. */
  #build(current: number): Promise<void> {
    this.#building ??= (async () => {
      const saved = await ipc.searchTextGet(this.bookId).catch(() => [] as [number, string][])
      this.timings.saved = Math.round(performance.now() - this.#started)
      const known = saved.filter(([i]) => this.#sections.includes(i))
      if (known.length) {
        for (const [i] of known) this.#extracted.add(i)
        this.#post({ type: 'add', chapters: known.map(([index, text]) => ({ index, text })) })
      }
      const todo = searchOrder(
        this.#sections.filter((i) => !this.#extracted.has(i)).map((index) => ({ index })),
        current,
      ).map((x) => x.index)
      let batch: [number, string][] = []
      for (const index of todo) {
        if (this.#closed) return
        let text: string
        try {
          const doc = await this.book.sections[index].createDocument?.()
          const root = doc?.body ?? doc?.documentElement
          text = root ? extractText(root).text : ''
        } catch {
          text = '' // a damaged chapter is searched as empty (E3)
        }
        this.#extracted.add(index)
        this.#post({ type: 'add', chapters: [{ index, text }] })
        batch.push([index, text])
        if (batch.length >= SAVE_BATCH) {
          void ipc.searchTextPut(this.bookId, batch).catch(() => {})
          batch = []
        }
        // Yield, so typing and page turns stay responsive while indexing (F3).
        await new Promise((r) => setTimeout(r, 0))
      }
      if (batch.length && !this.#closed) await ipc.searchTextPut(this.bookId, batch).catch(() => {})
    })()
    return this.#building
  }
}
