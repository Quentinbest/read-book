import { describe, expect, it } from 'vitest'
import { Annotations, UndoStack } from './annotations.svelte'
import type { AnnotationRow } from '../lib/annotations/model'
import { WriteQueue } from '../app/writes'
import { MessageQueue } from '../lib/reader/messages'

function setup() {
  const saved = new Map<string, AnnotationRow>()
  const deleted = new Set<string>()
  const log: string[] = []
  let now = 1000
  const store = new Annotations({
    bookId: 'b1',
    contentHash: 'h2',
    now: () => now++,
    writes: { write: (_key, run) => run() },
    api: {
      save: async (row) => {
        saved.set(row.id, row)
        deleted.delete(row.id)
        log.push(`save ${row.color} ${row.note ?? ''}`.trim())
      },
      delete: async (id) => {
        deleted.add(id)
        log.push('delete')
      },
    },
  })
  return { store, saved, deleted, log }
}

const at = (cfi: string, exact = 'text') => ({ cfi, quote: { exact, prefix: '', suffix: '' } })

describe('B4 highlight edge cases', () => {
  it('selecting an existing highlight’s exact range changes its colour', () => {
    const { store } = setup()
    const first = store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'yellow')
    const again = store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'blue')
    expect(store.items).toHaveLength(1)
    expect(again.annotation.id).toBe(first.annotation.id)
    expect(again.previousColor).toBe('yellow')
    expect(store.items[0].color).toBe('blue')
  })

  it('a partial overlap makes a separate highlight', () => {
    const { store } = setup()
    store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'yellow')
    store.highlight(at('epubcfi(/6/4!/4/2,/1:5,/1:20)'), 'green')
    expect(store.items.map((a) => a.color)).toEqual(['yellow', 'green'])
  })

  it('H uses the last colour chosen', () => {
    const { store } = setup()
    expect(store.lastColor).toBe('yellow')
    store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'rose')
    expect(store.lastColor).toBe('rose')
  })
})

describe('A6 notes', () => {
  it('an empty note is discarded and the highlight kept', async () => {
    const { store, saved } = setup()
    const { annotation } = store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'green')
    await store.setNote(annotation.id, 'A thought')
    expect(saved.get(annotation.id)?.note).toBe('A thought')
    await store.setNote(annotation.id, '   \n ')
    expect(store.get(annotation.id)?.note).toBeNull()
    expect(saved.get(annotation.id)?.note).toBeNull()
    expect(store.items).toHaveLength(1)
  })

  it('E5: the same text again after a failed save does not drop that save', async () => {
    // Typing then deleting a character sends the unchanged note while its failed
    // save is still pending; Retry must then save the note, not a no-op.
    const writes = new WriteQueue(new MessageQueue({ now: () => 0 }))
    const saved = new Map<string, AnnotationRow>()
    let full = false
    const store = new Annotations({
      bookId: 'b1',
      contentHash: 'h2',
      writes,
      api: {
        save: async (row) => {
          if (full) throw { kind: 'saveFailed', message: 'database or disk is full' }
          saved.set(row.id, row)
        },
        delete: async () => {},
      },
    })
    const { annotation } = store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'green')
    await writes.idle()
    full = true
    await store.setNote(annotation.id, 'A thought')
    await store.setNote(annotation.id, 'A thought')
    expect(writes.pendingKeys).toEqual([`annotation:${annotation.id}`])
    full = false
    await writes.retry()
    expect(saved.get(annotation.id)?.note).toBe('A thought')
    expect(writes.pendingKeys).toEqual([])
  })
})

describe('A7, B5 delete and Undo', () => {
  it('Undo restores the exact annotation, once', () => {
    const { store, saved, deleted } = setup()
    const { annotation } = store.highlight(at('epubcfi(/6/4!/4/2,/1:0,/1:10)'), 'green')
    const exact = store.get(annotation.id)!
    const undo = new UndoStack()
    const gone = store.remove(annotation.id)!
    const run = undo.push(() => store.restore(gone))
    expect(store.items).toHaveLength(0)
    expect(deleted.has(annotation.id)).toBe(true)
    expect(undo.undo()).toBe(true)
    expect(store.get(annotation.id)).toEqual(exact)
    expect(saved.get(annotation.id)?.updated_at).toBe(exact.updatedAt)
    expect(deleted.has(annotation.id)).toBe(false)
    run() // the message's Undo after ⌘Z: nothing more
    expect(store.items).toHaveLength(1)
    expect(undo.canUndo).toBe(false)
  })

  it('undoes the most recent change first', () => {
    const undo = new UndoStack()
    const done: number[] = []
    undo.push(() => done.push(1))
    const second = undo.push(() => done.push(2))
    second()
    undo.undo()
    expect(done).toEqual([2, 1])
    expect(undo.undo()).toBe(false)
  })
})

describe('B3 re-anchoring', () => {
  it('lists annotations from another edition, and places or reports them', () => {
    const { store } = setup()
    store.load([
      {
        id: 'a',
        book_id: 'b1',
        anchored_content_hash: 'h1',
        color: 'blue',
        cfi_range: 'old',
        quote_exact: 'x',
        quote_prefix: '',
        quote_suffix: '',
        note: null,
        created_at: 1,
        updated_at: 2,
        anchor_status: 'anchored',
      },
      {
        id: 'b',
        book_id: 'b1',
        anchored_content_hash: 'h1',
        color: 'rose',
        cfi_range: 'old2',
        quote_exact: 'y',
        quote_prefix: '',
        quote_suffix: '',
        note: 'n',
        created_at: 1,
        updated_at: 2,
        anchor_status: 'anchored',
      },
      {
        id: 'c',
        book_id: 'b1',
        anchored_content_hash: 'h2',
        color: 'green',
        cfi_range: 'old3',
        quote_exact: 'z',
        quote_prefix: '',
        quote_suffix: '',
        note: null,
        created_at: 1,
        updated_at: 2,
        anchor_status: 'reanchor',
      },
    ])
    expect(store.stale.map((a) => a.id)).toEqual(['a', 'b', 'c'])
    store.reanchor('c', { cfi: 'new3', quote: { exact: 'z', prefix: '', suffix: '' } })
    store.reanchor('a', { cfi: 'new', quote: { exact: 'x', prefix: 'p', suffix: '' } })
    store.reanchor('b', null)
    expect(store.get('a')).toMatchObject({
      cfi: 'new',
      status: 'anchored',
      anchoredContentHash: 'h2',
      updatedAt: 2,
    })
    expect(store.get('b')?.status).toBe('unplaced')
    expect(store.placed.map((a) => a.id)).toEqual(['a', 'c'])
    expect(store.unplaced.map((a) => a.id)).toEqual(['b'])
    expect(store.stale).toEqual([])
  })
})
