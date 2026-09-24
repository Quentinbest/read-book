import { describe, expect, it } from 'vitest'
import { AXIS_LOCK_PX, NOTCH_COOLDOWN_MS, WheelTurns, type WheelSample } from './wheel'

/** A trackpad swipe: rising then falling finger deltas, then an exponential momentum tail. */
function swipe(
  t0: number,
  axis: 'x' | 'y',
  sign = 1,
  peak = 30,
  momentumFrames = 60,
): WheelSample[] {
  const out: WheelSample[] = []
  let t = t0
  const finger = [4, 10, 18, peak, peak, 22, 14]
  for (const d of finger) out.push(sample(axis, sign * d, (t += 8)))
  let d = 12
  for (let i = 0; i < momentumFrames && d > 0.5; i++) {
    out.push(sample(axis, sign * d, (t += 16)))
    d *= 0.9
  }
  return out
}

const sample = (axis: 'x' | 'y', d: number, t: number): WheelSample =>
  axis === 'x' ? { dx: d, dy: 0, t } : { dx: 0, dy: d, t }

const run = (samples: WheelSample[]) => {
  const w = new WheelTurns()
  return samples.map((s) => w.feed(s)).filter(Boolean)
}

describe('I1 notched wheel', () => {
  it('one notch is one page, with a 250 ms cooldown', () => {
    const notch = (t: number, down = true): WheelSample => ({
      dx: 0,
      dy: down ? 4 : -4,
      t,
      wheelDeltaY: down ? -120 : 120,
    })
    expect(run([notch(0), notch(NOTCH_COOLDOWN_MS - 1), notch(NOTCH_COOLDOWN_MS + 10)])).toEqual([
      'next',
      'next',
    ])
    expect(run([notch(0, false)])).toEqual(['prev'])
  })
})

describe('I2/I3/I5 trackpad: one turn per gesture, momentum ignored', () => {
  it('a vertical swipe with a long momentum tail turns exactly once', () => {
    expect(run(swipe(0, 'y'))).toEqual(['next'])
  })

  it('a horizontal swipe turns once, in its direction', () => {
    expect(run(swipe(0, 'x', -1))).toEqual(['prev'])
  })

  it('two separate swipes turn twice', () => {
    const a = swipe(0, 'y')
    const b = swipe(a[a.length - 1].t + 300, 'y')
    expect(run([...a, ...b])).toEqual(['next', 'next'])
  })

  it('a new swipe during the momentum tail turns again', () => {
    const a = swipe(0, 'y', 1, 30, 20) // tail cut short: the next swipe starts while it decays
    const tailEnd = a[a.length - 1].t
    const b = swipe(tailEnd + 16, 'y')
    expect(run([...a, ...b])).toEqual(['next', 'next'])
  })

  it('scrolling less than 80 px does not turn', () => {
    const small = [3, 5, 8, 8, 6, 4, 2].map((d, i) => sample('y', d, i * 8))
    expect(run(small)).toEqual([])
  })
})

describe('I4 axis lock', () => {
  it('the first 12 px decide the axis; the other axis is ignored after that', () => {
    const samples: WheelSample[] = []
    let t = 0
    // Mostly horizontal start…
    for (let i = 0; i < 3; i++) samples.push({ dx: 6, dy: 1, t: (t += 8) })
    // …then a vertical drift that alone would pass 80 px.
    for (let i = 0; i < 10; i++) samples.push({ dx: 2, dy: 12, t: (t += 8) })
    expect(AXIS_LOCK_PX).toBe(12)
    expect(run(samples)).toEqual([])
  })
})
