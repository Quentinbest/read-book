import { describe, expect, it } from 'vitest'
import { SearchSession, type SearchEvent } from './session'

const chapter = (index: number, text: string) => ({ index, text })
const found = (events: SearchEvent[]) =>
  events.filter((e) => e.type === 'chapter').map((e) => (e.type === 'chapter' ? e.index : -1))

describe('F3 streaming search over chapters that are still arriving', () => {
  it('scans in the given order, holding back later chapters until earlier ones arrive', () => {
    const s = new SearchSession()
    s.add([chapter(3, 'water here'), chapter(5, 'more water')])
    // Current chapter 3 first, then 4, 5, wrapping to 1, 2.
    const first = s.search(1, 'water', [3, 4, 5, 1, 2])
    expect(found(first)).toEqual([3])
    expect(first.at(-1)).toEqual({ type: 'progress', id: 1, searched: 1, total: 5 })
    // Chapter 4 arrives: it and the already-indexed 5 are scanned, in order.
    const second = s.add([chapter(4, 'no match')])
    expect(found(second)).toEqual([5])
    expect(second.at(-1)).toEqual({ type: 'progress', id: 1, searched: 3, total: 5 })
    const last = s.add([chapter(1, 'water'), chapter(2, 'Water again, WATER')])
    expect(found(last)).toEqual([1, 2])
    expect(last.at(-1)).toEqual({ type: 'done', id: 1, searched: 5, total: 5 })
  })

  it('finds everything at once when the book is already indexed', () => {
    const s = new SearchSession()
    s.add([chapter(0, 'Il était'), chapter(1, 'etait'), chapter(2, 'x')])
    const events = s.search(7, 'etait', [0, 1, 2])
    expect(found(events)).toEqual([0, 1])
    expect(events.at(-1)?.type).toBe('done')
  })

  it('replaces a running search, and ends at once below the minimum length', () => {
    const s = new SearchSession()
    s.search(1, 'water', [0, 1])
    expect(s.search(2, 'w', [0, 1])).toEqual([{ type: 'done', id: 2, searched: 0, total: 0 }])
    expect(s.add([chapter(0, 'water')])).toEqual([])
  })
})
