import { describe, expect, it } from 'vitest'
import { PageCounter } from './pages'

describe('B1 book-wide page numbers', () => {
  it('estimates uncounted sections and is exact once every section is counted', () => {
    const p = new PageCounter([4000, 0, 8000, 2000])
    expect(p.exact).toBe(false)
    p.count(0, 2)
    expect(p.has(0)).toBe(true)
    expect(p.has(2)).toBe(false)
    // 2000 characters per page so far: section 2 is estimated at 4 pages.
    expect(p.pageNumber(3, 1)).toBe(2 + 4 + 1)
    p.count(2, 5)
    p.count(3, 1)
    expect(p.exact).toBe(true) // the empty section needs no count
    expect(p.pageNumber(3, 1)).toBe(2 + 5 + 1)
    expect(p.total).toBe(8)
  })

  it('forgets counts after a reflow', () => {
    const p = new PageCounter([1000])
    p.count(0, 3)
    p.reset()
    expect(p.has(0)).toBe(false)
    expect(p.exact).toBe(false)
  })
})
