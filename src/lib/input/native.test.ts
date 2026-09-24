import { describe, expect, it } from 'vitest'
import { NativeTurns, PHASE, type NativeScroll } from './native'

const ev = (p: Partial<NativeScroll>): NativeScroll => ({
  precise: true,
  phase: 0,
  momentum: 0,
  dx: 0,
  dy: 0,
  x: 0,
  y: 0,
  t: 0,
  ...p,
})

const run = (events: NativeScroll[]) => {
  const n = new NativeTurns()
  return events.map((e) => n.feed(e)).filter(Boolean)
}

/** An accelerated wheel roll as macOS delivers it (Spike B run 1): a burst of growing deltas. */
const roll = (t0: number, sign = -1) =>
  [4, 39, 160, 225, 256].map((d, i) => ev({ precise: false, dy: sign * d, t: t0 + i * 15 }))

describe('I1 wheel (not precise)', () => {
  it('one roll turns one page, whatever the acceleration', () => {
    expect(run(roll(0))).toEqual(['next'])
    expect(run(roll(0, 1))).toEqual(['prev'])
  })

  it('separate rolls turn once each', () => {
    const rolls = Array.from({ length: 25 }, (_, i) => roll(i * 400)).flat()
    expect(run(rolls)).toHaveLength(25)
  })

  it('a long continuous roll turns at most every 250 ms', () => {
    const long = Array.from({ length: 50 }, (_, i) => ev({ precise: false, dy: -40, t: i * 20 }))
    // 1 s of rolling: turns at 0, 250, 500, 750 ms
    expect(run(long)).toHaveLength(4)
  })
})

/** A trackpad swipe with phases, then its momentum tail. */
function swipe(t0: number, axis: 'x' | 'y', sign = -1): NativeScroll[] {
  const d = (v: number) => (axis === 'x' ? { dx: sign * v } : { dy: sign * v })
  const out = [ev({ phase: PHASE.began, ...d(2), t: t0 })]
  for (const [i, v] of [10, 25, 30, 20, 8].entries())
    out.push(ev({ phase: PHASE.changed, ...d(v), t: t0 + 8 * (i + 1) }))
  out.push(ev({ phase: PHASE.ended, t: t0 + 60 }))
  let v = 20
  for (let i = 0; v > 0.5; i++, v *= 0.9)
    out.push(ev({ momentum: i === 0 ? PHASE.began : PHASE.changed, ...d(v), t: t0 + 76 + 16 * i }))
  out.push(ev({ momentum: PHASE.ended, t: t0 + 2000 }))
  return out
}

describe('I2/I3/I5 precise devices', () => {
  it('a swipe turns once; its momentum never turns', () => {
    expect(run(swipe(0, 'y'))).toEqual(['next'])
    expect(run(swipe(0, 'x', 1))).toEqual(['prev'])
  })

  it('a new swipe during the previous momentum turns again', () => {
    const a = swipe(0, 'y').slice(0, 20) // momentum interrupted by the next Began
    expect(run([...a, ...swipe(400, 'y')])).toEqual(['next', 'next'])
  })

  it('a short scroll does not turn', () => {
    const short = [
      ev({ phase: PHASE.began, dy: -3 }),
      ev({ phase: PHASE.changed, dy: -20, t: 8 }),
      ev({ phase: PHASE.ended, t: 16 }),
    ]
    expect(run(short)).toEqual([])
  })

  it('I4: the first 12 px lock the axis', () => {
    const diag = [
      ev({ phase: PHASE.began, dx: -8, dy: -1 }),
      ev({ phase: PHASE.changed, dx: -8, dy: -2, t: 8 }),
      ...Array.from({ length: 8 }, (_, i) =>
        ev({ phase: PHASE.changed, dx: -2, dy: -15, t: 16 + 8 * i }),
      ),
      ev({ phase: PHASE.ended, t: 100 }),
    ]
    expect(run(diag)).toEqual([])
  })
})
