// Message queue (plan M1–M4; design Screen 12, P§8, P§15).
//
// One message shows at a time, bottom centre, newest first. Timers run 10 s and
// pause while the message is pointed at or focused. A dismissed or expired
// message stays reachable under ⌘K › Recently closed, with its action intact,
// so nothing that offers an action disappears beyond the reader's reach (M4).
//
// Time is injected (`now`) so the rules are testable without real timers; the
// UI calls `tick()` from a timer and renders `current`.

export const MESSAGE_TIMEOUT_MS = 10_000
/** B13 recommended default (provisional): Recently closed keeps the last 20 for the session. */
export const RECENTLY_CLOSED_LIMIT = 20

export interface MessageAction {
  label: string
  /** Shortcut shown next to the action, e.g. ⌘Z or ⌘[. */
  shortcut?: string
  run(): void
}

export interface MessageInput {
  text: string
  action?: MessageAction
  /** Assertive only for failures the reader must act on (for example a failed save). */
  politeness?: 'polite' | 'assertive'
  /** Messages that must stay until handled (E5 save failure) have no timer. */
  persistent?: boolean
}

export interface Message extends Required<
  Pick<MessageInput, 'text' | 'politeness' | 'persistent'>
> {
  id: number
  action?: MessageAction
  /** Time left on the timer, frozen while paused. */
  remainingMs: number
}

export interface Announcer {
  announce(text: string, politeness: 'polite' | 'assertive'): void
}

export class MessageQueue {
  #now: () => number
  #announcer?: Announcer
  #pending: Message[] = []
  #current: Message | null = null
  #shownAt = 0
  #paused = new Set<'hover' | 'focus'>()
  #recentlyClosed: Message[] = []
  #nextId = 1
  #listeners = new Set<() => void>()

  constructor(options: { now?: () => number; announcer?: Announcer } = {}) {
    this.#now = options.now ?? (() => performance.now())
    this.#announcer = options.announcer
  }

  get current(): Message | null {
    return this.#current
  }

  get pending(): readonly Message[] {
    return this.#pending
  }

  /** Newest first (M3: ⌘K › Recently closed). */
  get recentlyClosed(): readonly Message[] {
    return this.#recentlyClosed
  }

  get paused(): boolean {
    return this.#paused.size > 0
  }

  subscribe(listener: () => void): () => void {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  /** Show a message now; one already showing goes back to the queue with its remaining time (M1). */
  push(input: MessageInput): Message {
    const message: Message = {
      id: this.#nextId++,
      text: input.text,
      action: input.action,
      politeness: input.politeness ?? 'polite',
      persistent: input.persistent ?? false,
      remainingMs: MESSAGE_TIMEOUT_MS,
    }
    if (this.#current) {
      this.#freeze()
      this.#pending.unshift(this.#current)
    }
    this.#show(message)
    return message
  }

  /** Pointer over the message or focus inside it pauses the timer (M2). */
  setPaused(reason: 'hover' | 'focus', paused: boolean) {
    const was = this.paused
    if (paused) this.#paused.add(reason)
    else this.#paused.delete(reason)
    if (!was && this.paused) this.#freeze()
    if (was && !this.paused) this.#shownAt = this.#now()
    this.#emit()
  }

  /** Advance time: expire the current message if its timer has run out. */
  tick() {
    const m = this.#current
    if (!m || m.persistent || this.paused) return
    if (this.#now() - this.#shownAt >= m.remainingMs) this.#close(m)
  }

  /** The reader closed the message. */
  dismiss(id: number) {
    if (this.#current?.id === id) this.#close(this.#current)
    else {
      const i = this.#pending.findIndex((m) => m.id === id)
      if (i >= 0) this.#remember(this.#pending.splice(i, 1)[0])
      this.#emit()
    }
  }

  /** Run a message's action (from its button, its shortcut or Recently closed) and retire it. */
  act(id: number): boolean {
    const all = [this.#current, ...this.#pending, ...this.#recentlyClosed]
    const m = all.find((x) => x?.id === id)
    if (!m?.action) return false
    m.action.run()
    this.#recentlyClosed = this.#recentlyClosed.filter((x) => x.id !== id)
    if (this.#current?.id === id) {
      this.#current = null
      this.#showNext()
    } else {
      this.#pending = this.#pending.filter((x) => x.id !== id)
    }
    this.#emit()
    return true
  }

  #show(m: Message) {
    this.#current = m
    this.#shownAt = this.#now()
    this.#paused.clear()
    this.#announcer?.announce(m.action ? `${m.text}. ${m.action.label}` : m.text, m.politeness)
    this.#emit()
  }

  #freeze() {
    const m = this.#current
    if (m && !m.persistent)
      m.remainingMs = Math.max(0, m.remainingMs - (this.#now() - this.#shownAt))
    this.#shownAt = this.#now()
  }

  #close(m: Message) {
    this.#current = null
    this.#remember(m)
    this.#showNext()
    this.#emit()
  }

  #showNext() {
    const next = this.#pending.shift()
    if (next) this.#show(next)
  }

  #remember(m: Message) {
    if (!m.action) return
    this.#recentlyClosed = [m, ...this.#recentlyClosed].slice(0, RECENTLY_CLOSED_LIMIT)
  }

  #emit() {
    for (const l of this.#listeners) l()
  }
}
