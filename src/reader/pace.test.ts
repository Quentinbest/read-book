import { describe, expect, it } from 'vitest'
import { DEFAULT_WPM, ReadingPace } from './pace'
import { PageCounter } from './pages'

describe('B2 reading pace', () => {
  it('shows nothing until 5 minutes have been read', () => {
    const p = new ReadingPace()
    expect(p.minutesLeft(60_000)).toBeNull()
    for (let i = 0; i < 9; i++) p.record(30_000, 1500, i * 30_000) // 4.5 min
    expect(p.minutesLeft(60_000)).toBeNull()
    p.record(30_000, 1500, 300_000)
    expect(p.minutesLeft(60_000)).not.toBeNull()
  })

  it('uses the median pace and ignores dwells under 2 s or over 2 min', () => {
    const p = new ReadingPace()
    expect(p.wpm).toBe(DEFAULT_WPM)
    // 1500 chars = 250 words in 60 s → 250 wpm; one 300 wpm outlier and ignored dwells.
    for (let i = 0; i < 6; i++) p.record(60_000, 1500, i)
    p.record(50_000, 1500, 7)
    p.record(1_000, 1500, 8)
    p.record(600_000, 1500, 9)
    expect(p.wpm).toBe(250)
    // 15,000 chars left = 2,500 words = 10 min at 250 wpm.
    expect(p.minutesLeft(15_000)).toBe(10)
    expect(p.minutesLeft(600)).toBeNull() // under a minute
  })

  it('round-trips through its saved form', () => {
    const p = new ReadingPace({ wpm: 300, readMs: 400_000 })
    expect(new ReadingPace(p.toJSON()).minutesLeft(18_000)).toBe(10)
  })
})

describe('B1 page numbers', () => {
  it('estimates from sizes until sections are counted, then is exact', () => {
    const c = new PageCounter([4000, 8000, 0, 2000])
    c.count(0, 2) // 2000 chars per page
    expect(c.total).toBe(2 + 4 + 0 + 1)
    expect(c.exact).toBe(false)
    expect(c.pageNumber(1, 3)).toBe(5)
    c.count(1, 5)
    c.count(3, 2)
    expect(c.exact).toBe(true)
    expect(c.total).toBe(9)
    expect(c.pageNumber(3, 1)).toBe(8)
  })
})
