import { describe, expect, it } from 'vitest'
import { MessageQueue } from '../lib/reader/messages'
import { SAVE_FAILED_TEXT, WriteQueue } from './writes'

const diskFull = { kind: 'saveFailed', message: 'database or disk is full' }

function setup() {
  const messages = new MessageQueue({ now: () => 0 })
  const queue = new WriteQueue(messages)
  const saved: string[] = []
  let full = false
  const write = (key: string, value: string) =>
    queue.write(key, async () => {
      if (full) throw diskFull
      saved.push(`${key}=${value}`)
    })
  return { messages, queue, saved, write, setFull: (v: boolean) => (full = v) }
}

describe('E5 write failures', () => {
  it('writes in order when the disk is fine', async () => {
    const { saved, write } = setup()
    await write('position', 'a')
    await write('note:1', 'hello')
    expect(saved).toEqual(['position=a', 'note:1=hello'])
  })

  it('a failed write shows one persistent Retry message and keeps every pending write', async () => {
    const { messages, queue, saved, write, setFull } = setup()
    setFull(true)
    await write('note:1', 'draft')
    await write('position', 'p1')
    await write('position', 'p2') // replaces the pending p1
    expect(saved).toEqual([])
    expect(queue.pendingKeys).toEqual(['note:1', 'position'])
    expect(messages.current?.text).toBe(SAVE_FAILED_TEXT)
    expect(messages.current?.persistent).toBe(true)
    expect(messages.current?.action?.label).toBe('Retry')
    expect(messages.pending).toHaveLength(0) // one message, not one per write
  })

  it('Retry succeeds once space returns, and the message goes away', async () => {
    const { messages, queue, saved, write, setFull } = setup()
    setFull(true)
    await write('note:1', 'draft')
    await write('position', 'p2')
    setFull(false)
    messages.act(messages.current!.id)
    await queue.retry() // the action's retry is async; await a flush to finish
    expect(saved).toEqual(['note:1=draft', 'position=p2'])
    expect(queue.pendingKeys).toEqual([])
    expect(messages.current).toBeNull()
  })

  it('Retry while the disk is still full keeps the data and the message', async () => {
    const { messages, queue, saved, write, setFull } = setup()
    setFull(true)
    await write('note:1', 'draft')
    await queue.retry()
    expect(saved).toEqual([])
    expect(queue.pendingKeys).toEqual(['note:1'])
    expect(messages.current?.text).toBe(SAVE_FAILED_TEXT)
  })
})

describe('E5 transient failures', () => {
  it('a write that fails without a disk error is retried before the Retry message', async () => {
    const messages = new MessageQueue({ now: () => 0 })
    const queue = new WriteQueue(messages)
    let calls = 0
    await queue.write('position', async () => {
      if (++calls < 2) throw new TypeError('Load failed')
    })
    expect(calls).toBe(2)
    expect(queue.pendingKeys).toEqual([])
    expect(messages.current).toBeNull()
    expect(queue.lastError?.error).toBe('TypeError: Load failed')
  })

  it('gives up after a few attempts, keeping the write', async () => {
    const messages = new MessageQueue({ now: () => 0 })
    const queue = new WriteQueue(messages)
    let calls = 0
    await queue.write('position', async () => {
      calls++
      throw new TypeError('Load failed')
    })
    expect(calls).toBe(3)
    expect(queue.pendingKeys).toEqual(['position'])
    expect(messages.current?.text).toBe(SAVE_FAILED_TEXT)
  })
})
