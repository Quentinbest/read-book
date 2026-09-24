// Back history (plan N1): every jump pushes the location it left; Back pops it.
// Per book and in memory (B13 recommended default, provisional: last 50 jumps).

export const HISTORY_LIMIT = 50

export interface HistoryEntry {
  /** EPUB CFI of the location left by the jump. */
  cfi: string
  /** What caused the jump: contents, search, link, annotation, percent, goto. */
  source: 'contents' | 'search' | 'link' | 'annotation' | 'percent' | 'goto' | 'pages'
}

export class LocationHistory {
  #entries: HistoryEntry[] = []
  #limit: number

  constructor(limit = HISTORY_LIMIT) {
    this.#limit = limit
  }

  get canGoBack(): boolean {
    return this.#entries.length > 0
  }

  /** The location Back would return to (drives the “Back to page N” chip, N2). */
  peek(): HistoryEntry | null {
    return this.#entries.at(-1) ?? null
  }

  /** Record the location being left by a jump. Consecutive duplicates collapse. */
  push(entry: HistoryEntry) {
    if (this.#entries.at(-1)?.cfi === entry.cfi) return
    this.#entries.push(entry)
    if (this.#entries.length > this.#limit)
      this.#entries.splice(0, this.#entries.length - this.#limit)
  }

  back(): HistoryEntry | null {
    return this.#entries.pop() ?? null
  }

  clear() {
    this.#entries = []
  }

  get size(): number {
    return this.#entries.length
  }
}
