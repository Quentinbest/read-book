import { describe, expect, it, vi } from 'vitest'
import { LocationHistory } from './history'
import { MESSAGE_TIMEOUT_MS, MessageQueue, RECENTLY_CLOSED_LIMIT } from './messages'

function setup() {
  let t = 0
  const announced: string[] = []
  const q = new MessageQueue({
    now: () => t,
    announcer: { announce: (text, p) => announced.push(`${p}:${text}`) },
  })
  const advance = (ms: number) => {
    t += ms
    q.tick()
  }
  return { q, advance, announced }
}

const undo = (run = vi.fn()) => ({ label: 'Undo', shortcut: '⌘Z', run })

describe('M1 one message at a time, newest first, announced', () => {
  it('a new message replaces the current one, which returns afterwards', () => {
    const { q, advance } = setup()
    const a = q.push({ text: 'Highlight deleted', action: undo() })
    advance(4000)
    const b = q.push({ text: 'Back to page 43' })
    expect(q.current?.id).toBe(b.id)
    expect(q.pending.map((m) => m.id)).toEqual([a.id])
    advance(MESSAGE_TIMEOUT_MS)
    expect(q.current?.id).toBe(a.id)
    // a had 6 s left
    advance(5999)
    expect(q.current?.id).toBe(a.id)
    advance(1)
    expect(q.current).toBeNull()
  })

  it('announces each message with its action to screen readers', () => {
    const { q, announced } = setup()
    q.push({ text: 'Highlight deleted', action: undo() })
    q.push({
      text: "Couldn't save notes to disk",
      action: { label: 'Retry', run() {} },
      politeness: 'assertive',
      persistent: true,
    })
    expect(announced).toEqual([
      'polite:Highlight deleted. Undo',
      "assertive:Couldn't save notes to disk. Retry",
    ])
  })
})

describe('M2 10 s timers that pause on hover or focus', () => {
  it('expires after 10 s', () => {
    const { q, advance } = setup()
    q.push({ text: 'x' })
    advance(MESSAGE_TIMEOUT_MS - 1)
    expect(q.current).not.toBeNull()
    advance(1)
    expect(q.current).toBeNull()
  })

  it('pauses while pointed at or focused, and resumes with the remaining time', () => {
    const { q, advance } = setup()
    q.push({ text: 'x' })
    advance(3000)
    q.setPaused('hover', true)
    advance(60_000)
    q.setPaused('focus', true)
    q.setPaused('hover', false)
    advance(60_000)
    expect(q.current).not.toBeNull()
    q.setPaused('focus', false)
    advance(6999)
    expect(q.current).not.toBeNull()
    advance(1)
    expect(q.current).toBeNull()
  })

  it('persistent messages never expire', () => {
    const { q, advance } = setup()
    q.push({
      text: "Couldn't save notes to disk",
      persistent: true,
      action: { label: 'Retry', run() {} },
    })
    advance(10 * MESSAGE_TIMEOUT_MS)
    expect(q.current?.text).toBe("Couldn't save notes to disk")
  })
})

describe('M3/M4 dismissed actions stay reachable in Recently closed', () => {
  it('an expired Undo can still be run from Recently closed', () => {
    const { q, advance } = setup()
    const run = vi.fn()
    const m = q.push({ text: 'Highlight deleted', action: undo(run) })
    advance(MESSAGE_TIMEOUT_MS)
    expect(q.recentlyClosed.map((x) => x.id)).toEqual([m.id])
    expect(q.act(m.id)).toBe(true)
    expect(run).toHaveBeenCalledOnce()
    expect(q.recentlyClosed).toHaveLength(0)
  })

  it('dismissing keeps it too; messages without actions are not kept', () => {
    const { q } = setup()
    const a = q.push({ text: 'Highlight deleted', action: undo() })
    q.dismiss(a.id)
    q.dismiss(q.push({ text: 'Plain info' }).id)
    expect(q.recentlyClosed.map((x) => x.text)).toEqual(['Highlight deleted'])
  })

  it('keeps the newest 20', () => {
    const { q } = setup()
    for (let i = 0; i < RECENTLY_CLOSED_LIMIT + 5; i++)
      q.dismiss(q.push({ text: `m${i}`, action: undo() }).id)
    expect(q.recentlyClosed).toHaveLength(RECENTLY_CLOSED_LIMIT)
    expect(q.recentlyClosed[0].text).toBe(`m${RECENTLY_CLOSED_LIMIT + 4}`)
  })

  it('acting on the current message shows the next one', () => {
    const { q } = setup()
    const a = q.push({ text: 'a', action: undo() })
    const b = q.push({ text: 'b', action: undo() })
    q.act(b.id)
    expect(q.current?.id).toBe(a.id)
  })
})

describe('N1 Back history', () => {
  it('pushes the location left by each jump and pops it on Back', () => {
    const h = new LocationHistory()
    h.push({ cfi: 'a', source: 'contents' })
    h.push({ cfi: 'b', source: 'search' })
    expect(h.peek()?.cfi).toBe('b')
    expect(h.back()?.cfi).toBe('b')
    expect(h.back()?.cfi).toBe('a')
    expect(h.back()).toBeNull()
    expect(h.canGoBack).toBe(false)
  })

  it('collapses consecutive duplicates and keeps the newest 50', () => {
    const h = new LocationHistory()
    h.push({ cfi: 'a', source: 'link' })
    h.push({ cfi: 'a', source: 'link' })
    expect(h.size).toBe(1)
    for (let i = 0; i < 60; i++) h.push({ cfi: `c${i}`, source: 'goto' })
    expect(h.size).toBe(50)
    expect(h.peek()?.cfi).toBe('c59')
  })
})

describe('withdrawing a message whose action no longer applies', () => {
  it('removes it from the screen, the queue and Recently closed, and shows the next', () => {
    const { q, advance } = setup()
    const a = q.push({ text: 'Back to page 3', action: undo() })
    const b = q.push({ text: 'Back to page 9', action: undo() })
    q.withdraw(b.id) // showing
    expect(q.current?.id).toBe(a.id)
    advance(MESSAGE_TIMEOUT_MS)
    expect(q.recentlyClosed.map((m) => m.id)).toEqual([a.id])
    q.withdraw(a.id) // in Recently closed
    expect(q.current).toBeNull()
    expect(q.recentlyClosed).toEqual([])
    expect(q.act(a.id)).toBe(false)
  })
})

describe('Recently closed lists each message once', () => {
  it('keeps only the newest of identical messages', () => {
    const { q, advance } = setup()
    for (let i = 0; i < 3; i++) {
      q.push({ text: 'Resumed in I: Loomings', action: undo() })
      advance(MESSAGE_TIMEOUT_MS)
    }
    expect(q.recentlyClosed.length).toBe(1)
  })
})
