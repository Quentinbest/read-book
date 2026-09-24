// Write queue for user data (plan E5): nothing is dropped silently.
//
// Writes are keyed (a later write to the same key replaces a pending one, as a
// newer reading position replaces an older one). A write that fails with
// `saveFailed` stays queued and raises one persistent “Couldn't save notes to
// disk · Retry” message; Retry re-sends everything pending, in order.

import { t } from '../lib/strings/en'
import { isCommandError } from './ipc'
import type { MessageQueue } from '../lib/reader/messages'

export const SAVE_FAILED_TEXT = t.messages.saveFailed

interface Pending {
  key: string
  run: () => Promise<void>
}

export class WriteQueue {
  #pending: Pending[] = []
  #flushing: Promise<void> | null = null
  #messages: MessageQueue
  #failureMessageId: number | null = null
  #failed = false

  constructor(messages: MessageQueue) {
    this.#messages = messages
  }

  get pendingKeys(): string[] {
    return this.#pending.map((p) => p.key)
  }

  get failed(): boolean {
    return this.#failed
  }

  /** Queue a write. Resolves when it (or a newer write to the same key) is saved. */
  write(key: string, run: () => Promise<void>): Promise<void> {
    this.#pending = this.#pending.filter((p) => p.key !== key)
    this.#pending.push({ key, run })
    return this.#flush()
  }

  /** Retry everything pending (the message's Retry action). */
  retry(): Promise<void> {
    this.#failed = false
    return this.#flush()
  }

  /** One drain at a time; callers during a drain share its promise. */
  #flush(): Promise<void> {
    if (this.#failed) return Promise.resolve()
    this.#flushing ??= this.#drain().finally(() => (this.#flushing = null))
    return this.#flushing
  }

  async #drain(): Promise<void> {
    while (this.#pending.length) {
      const next = this.#pending[0]
      try {
        await next.run()
        // Only drop it if no newer write for the key replaced it meanwhile.
        if (this.#pending[0] === next) this.#pending.shift()
      } catch (e) {
        // Disk trouble (saveFailed) or a bug: either way keep the data and say so.
        if (!(isCommandError(e) && e.kind === 'saveFailed'))
          console.error('write failed', next.key, e)
        this.#fail()
        return
      }
    }
    this.#recovered()
  }

  #fail() {
    this.#failed = true
    if (this.#failureMessageId !== null && this.#messages.current?.id === this.#failureMessageId)
      return
    const m = this.#messages.push({
      text: SAVE_FAILED_TEXT,
      politeness: 'assertive',
      persistent: true,
      action: { label: t.messages.retry, run: () => void this.retry() },
    })
    this.#failureMessageId = m.id
  }

  #recovered() {
    if (this.#failureMessageId !== null) {
      this.#messages.dismiss(this.#failureMessageId)
      this.#failureMessageId = null
    }
  }
}
