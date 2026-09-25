import { describe, expect, it } from 'vitest'
import { openedAgo, openedLine } from './when'

const at = (y: number, m: number, d: number, h = 12, min = 0) =>
  new Date(y, m - 1, d, h, min).getTime()
const now = at(2026, 9, 25, 10)

describe('E6 opened times', () => {
  it('today, yesterday, days ago, then a date', () => {
    expect(openedAgo(at(2026, 9, 25, 1), now)).toBe('today')
    expect(openedAgo(at(2026, 9, 24, 23), now)).toBe('yesterday')
    expect(openedAgo(at(2026, 9, 22), now)).toBe('3 days ago')
    expect(openedAgo(at(2026, 8, 1), now)).toBe('1 Aug')
    expect(openedAgo(at(2025, 8, 1), now)).toBe('1 Aug 2025')
  })
  it('Continue reading names the time today and yesterday', () => {
    expect(openedLine(at(2026, 9, 25, 9, 40), now)).toBe('Opened today at 09:40')
    expect(openedLine(at(2026, 9, 24, 21, 40), now)).toBe('Opened yesterday at 21:40')
    expect(openedLine(at(2026, 9, 20), now)).toBe('Opened 5 days ago')
  })
})
