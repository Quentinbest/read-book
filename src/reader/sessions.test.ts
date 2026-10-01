import { describe, expect, it } from 'vitest'
import { ReadingSessions, SESSION_IDLE_MS, type ReadingSession } from './sessions'

function setup() {
  let now = 1_000_000
  const ended: ReadingSession[] = []
  const s = new ReadingSessions(
    (x) => ended.push(x),
    () => now,
  )
  return { s, ended, at: (ms: number) => (now += ms) }
}

describe('reading sessions (1.1)', () => {
  it('a session runs from the first move to the last', () => {
    const { s, ended, at } = setup()
    s.moved(0.1, false)
    at(30_000)
    s.moved(0.11, true)
    at(30_000)
    s.moved(0.12, true)
    s.end()
    expect(ended).toEqual([
      {
        startedAt: 1_000_000,
        endedAt: 1_060_000,
        activeSeconds: 60,
        startFraction: 0.1,
        endFraction: 0.12,
        pagesTurned: 2,
      },
    ])
  })

  it('a long pause ends the session where the reader stopped', () => {
    const { s, ended, at } = setup()
    s.moved(0.1, false)
    at(20_000)
    s.moved(0.2, true)
    at(SESSION_IDLE_MS + 1)
    s.moved(0.2, false)
    expect(ended).toHaveLength(1)
    expect(ended[0].endedAt - ended[0].startedAt).toBe(20_000)
    at(15_000)
    s.moved(0.25, true)
    s.end()
    expect(ended).toHaveLength(2)
    expect(ended[1].startFraction).toBe(0.2)
    expect(ended[1].activeSeconds).toBe(15)
  })

  it('a glance is not a session', () => {
    const { s, ended, at } = setup()
    s.moved(0.5, false)
    at(3000)
    s.moved(0.5, false)
    s.end()
    expect(ended).toEqual([])
    // Ending twice, or with nothing read, says nothing.
    s.end()
    expect(ended).toEqual([])
  })

  it('a quick page turn still counts', () => {
    const { s, ended, at } = setup()
    s.moved(0.5, false)
    at(2000)
    s.moved(0.51, true)
    s.end()
    expect(ended).toHaveLength(1)
  })
})
