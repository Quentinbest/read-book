// The open book's highlights and notes (A4–A9), kept in memory and written
// through the write queue (E5): every change is saved, and a failed save stays
// queued behind the “Couldn't save notes to disk · Retry” message.
//
// Writes for one annotation share a key, so a note typed quickly makes one
// write per pause, and a delete replaces a save still waiting.

import {
  COLORS,
  fromRow,
  toRow,
  type Annotation,
  type HighlightColor,
} from '../lib/annotations/model'
import type { TextQuote } from '../lib/annotations/anchor'

export interface AnnotationApi {
  save(row: ReturnType<typeof toRow>): Promise<void>
  delete(id: string): Promise<void>
}

export interface Writer {
  write(key: string, run: () => Promise<void>): Promise<void>
}

export class Annotations {
  /** Replaced whole on every change (raw state: the list is read, never mutated in place). */
  items = $state.raw<Annotation[]>([])
  /** A3: H applies the last-used colour. */
  lastColor = $state<HighlightColor>('yellow')

  #bookId: string
  #contentHash: string
  #writes: Writer
  #api: AnnotationApi
  #now: () => number

  constructor(o: {
    bookId: string
    contentHash: string
    writes: Writer
    api: AnnotationApi
    now?: () => number
  }) {
    this.#bookId = o.bookId
    this.#contentHash = o.contentHash
    this.#writes = o.writes
    this.#api = o.api
    this.#now = o.now ?? Date.now
  }

  load(rows: Parameters<typeof fromRow>[0][]) {
    this.items = rows.map(fromRow)
  }

  get(id: string): Annotation | undefined {
    return this.items.find((a) => a.id === id)
  }

  /** Highlights to draw: those placed in this edition of the book. */
  get placed(): Annotation[] {
    return this.items.filter((a) => a.status === 'anchored')
  }

  get unplaced(): Annotation[] {
    return this.items.filter((a) => a.status !== 'anchored')
  }

  /**
   * A4, B4: highlight a selection. Selecting an existing highlight's exact range
   * changes its colour; any other selection (a partial overlap too) is a new one.
   */
  highlight(
    at: { cfi: string; quote: TextQuote },
    color: HighlightColor,
  ): { annotation: Annotation; previousColor: HighlightColor | null } {
    this.lastColor = color
    const same = this.items.find((a) => a.status === 'anchored' && a.cfi === at.cfi)
    if (same) {
      const previousColor = same.color
      return { annotation: this.#update(same.id, { color }) ?? same, previousColor }
    }
    const now = this.#now()
    const annotation: Annotation = {
      id: crypto.randomUUID(),
      bookId: this.#bookId,
      anchoredContentHash: this.#contentHash,
      color,
      cfi: at.cfi,
      quote: at.quote,
      note: null,
      createdAt: now,
      updatedAt: now,
      status: 'anchored',
    }
    this.items = [...this.items, annotation]
    this.#save(annotation)
    return { annotation, previousColor: null }
  }

  setColor(id: string, color: HighlightColor): HighlightColor | null {
    const a = this.get(id)
    if (!a || a.color === color) return null
    this.lastColor = color
    this.#update(id, { color })
    return a.color
  }

  /** A6: an empty note is no note; the highlight stays. Resolves when saved. */
  setNote(id: string, text: string | null): Promise<void> {
    const note = text && text.trim() ? text : null
    const a = this.get(id)
    if (!a) return Promise.resolve()
    // Unchanged text still saves the current annotation: a no-op under the same
    // key would replace a pending (for example failed) save of it.
    if (a.note === note) return this.#save(a)
    return this.#save(this.#update(id, { note }, false)!)
  }

  /** A7: delete at once; the returned annotation is what Undo puts back. */
  remove(id: string): Annotation | null {
    const a = this.get(id)
    if (!a) return null
    this.items = this.items.filter((x) => x.id !== id)
    void this.#writes.write(`annotation:${id}`, () => this.#api.delete(id))
    return a
  }

  /** Undo (A7, B5): the exact annotation comes back. */
  restore(a: Annotation) {
    this.items = [...this.items.filter((x) => x.id !== a.id), a]
    this.#save(a)
  }

  /**
   * B3, A8: placed again in a changed book (or reported unplaced), or re-attached
   * by the reader (G11). Automatic placing is not an edit: `updatedAt` stays.
   */
  reanchor(id: string, to: { cfi: string; quote: TextQuote } | null, byReader = false) {
    const a = this.get(id)
    if (!a) return
    const patch: Partial<Annotation> = to
      ? { cfi: to.cfi, quote: to.quote, anchoredContentHash: this.#contentHash, status: 'anchored' }
      : { status: 'unplaced' }
    this.#update(id, patch, true, byReader)
  }

  /**
   * Annotations made against another edition of the file (B3): marked by the import
   * that replaced it, or (for files replaced before that marking) by their hash.
   */
  get stale(): Annotation[] {
    return this.items.filter(
      (a) =>
        a.status === 'reanchor' ||
        (a.anchoredContentHash !== this.#contentHash && a.status !== 'unplaced'),
    )
  }

  #update(
    id: string,
    patch: Partial<Annotation>,
    save = true,
    touch = true,
  ): Annotation | undefined {
    const a = this.get(id)
    if (!a) return
    const next = { ...a, ...patch, updatedAt: touch ? this.#now() : a.updatedAt }
    this.items = this.items.map((x) => (x.id === id ? next : x))
    if (save) this.#save(next)
    return next
  }

  #save(a: Annotation): Promise<void> {
    return this.#writes.write(`annotation:${a.id}`, () => this.#api.save(toRow(a)))
  }
}

/** Colour names as lists show them (A4: lists always name the colour). */
export const COLOR_NAMES: Record<HighlightColor, string> = {
  yellow: 'Yellow',
  green: 'Green',
  blue: 'Blue',
  rose: 'Rose',
}
export { COLORS }

/**
 * B5: what ⌘Z undoes — deleting a highlight or a note, and colour changes. Each
 * entry runs once, from ⌘Z, the message's Undo or ⌘K › Recently closed.
 */
export class UndoStack {
  #entries: { id: number; run: () => void }[] = []
  #next = 1

  push(run: () => void): () => void {
    const id = this.#next++
    let done = false
    const once = () => {
      if (done) return
      done = true
      this.#entries = this.#entries.filter((e) => e.id !== id)
      run()
    }
    this.#entries.push({ id, run: once })
    if (this.#entries.length > 50) this.#entries.shift()
    return once
  }

  get canUndo(): boolean {
    return this.#entries.length > 0
  }

  /** Undo the most recent change still undoable. */
  undo(): boolean {
    const last = this.#entries.at(-1)
    if (!last) return false
    last.run()
    return true
  }
}
