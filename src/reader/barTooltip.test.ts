import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TOOLTIP_REST_MS, TOOLTIP_WARM_MS, tooltipTiming, type Clock } from './barTooltip'

const clock: Clock = {
  now: () => Date.now(),
  set: (fn, ms) => setTimeout(fn, ms) as unknown as number,
  clear: (id) => clearTimeout(id),
}

describe('tooltipTiming (SB2)', () => {
  let shown: (string | null)[]
  const make = () => tooltipTiming<string>((t) => shown.push(t), clock)
  beforeEach(() => {
    vi.useFakeTimers()
    shown = []
  })
  afterEach(() => vi.useRealTimers())

  it('shows after the pointer rests, not before', () => {
    const tips = make()
    tips.enter('note')
    vi.advanceTimersByTime(TOOLTIP_REST_MS - 1)
    expect(tips.current).toBeNull()
    vi.advanceTimersByTime(1)
    expect(tips.current).toBe('note')
    expect(shown).toEqual(['note'])
  })

  it('never shows for a button the pointer left before the rest was over', () => {
    const tips = make()
    tips.enter('note')
    vi.advanceTimersByTime(200)
    tips.leave()
    vi.advanceTimersByTime(TOOLTIP_REST_MS)
    expect(tips.current).toBeNull()
    expect(shown).toEqual([])
  })

  it('moves to the next button at once while warm', () => {
    const tips = make()
    tips.enter('note')
    vi.advanceTimersByTime(TOOLTIP_REST_MS)
    tips.leave()
    vi.advanceTimersByTime(TOOLTIP_WARM_MS - 1)
    tips.enter('copy')
    expect(tips.current).toBe('copy')
    expect(shown).toEqual(['note', null, 'copy'])
  })

  it('waits again once the pointer has been away longer than the warm time', () => {
    const tips = make()
    tips.enter('note')
    vi.advanceTimersByTime(TOOLTIP_REST_MS)
    tips.leave()
    vi.advanceTimersByTime(TOOLTIP_WARM_MS + 1)
    tips.enter('copy')
    expect(tips.current).toBeNull()
    vi.advanceTimersByTime(TOOLTIP_REST_MS)
    expect(tips.current).toBe('copy')
  })

  it('shows at once on keyboard focus', () => {
    const tips = make()
    tips.focus('look-up')
    expect(tips.current).toBe('look-up')
  })

  it('a click or Esc hides it and ends the warm time', () => {
    const tips = make()
    tips.focus('note')
    tips.hide()
    expect(tips.current).toBeNull()
    tips.enter('copy')
    expect(tips.current).toBeNull()
    vi.advanceTimersByTime(TOOLTIP_REST_MS)
    expect(tips.current).toBe('copy')
  })

  it('hiding cancels a tooltip still waiting', () => {
    const tips = make()
    tips.enter('note')
    tips.hide()
    vi.advanceTimersByTime(TOOLTIP_REST_MS * 2)
    expect(shown).toEqual([])
  })
})
