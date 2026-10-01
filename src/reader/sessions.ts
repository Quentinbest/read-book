// Reading sessions (1.1, PROVISIONAL; plan §1.3 “reading-statistics session
// events”). Linen keeps no statistics itself; an extension with
// `reading.sessions` hears each session as it ends and can keep its own.
//
// A session starts when the place in the book first moves, grows with each move,
// and ends after a long pause, when the book is put away, or when Linen quits.
// A pause counts up to the idle limit only once: a reader who stops for an hour
// ends the session where they stopped.

/** No move for this long ends the session at the last move. */
export const SESSION_IDLE_MS = 5 * 60_000
/** Shorter sessions without a page turn are glances, not reading; they are dropped. */
export const SESSION_MIN_MS = 10_000

export interface ReadingSession {
  startedAt: number
  endedAt: number
  /** Time spent reading, without pauses beyond the idle limit. */
  activeSeconds: number
  startFraction: number
  endFraction: number
  pagesTurned: number
}

export class ReadingSessions {
  #current: ReadingSession | null = null
  #onEnd: (s: ReadingSession) => void
  #now: () => number

  constructor(onEnd: (s: ReadingSession) => void, now: () => number = Date.now) {
    this.#onEnd = onEnd
    this.#now = now
  }

  /** The place moved (a turn, a scroll, a jump). */
  moved(fraction: number, turned: boolean) {
    const now = this.#now()
    const s = this.#current
    if (s && now - s.endedAt > SESSION_IDLE_MS) this.end()
    if (!this.#current) {
      this.#current = {
        startedAt: now,
        endedAt: now,
        activeSeconds: 0,
        startFraction: fraction,
        endFraction: fraction,
        pagesTurned: 0,
      }
      return
    }
    const c = this.#current
    c.activeSeconds += (now - c.endedAt) / 1000
    c.endedAt = now
    c.endFraction = fraction
    if (turned) c.pagesTurned++
  }

  /** The book was put away (the library, another book, quitting). */
  end() {
    const s = this.#current
    this.#current = null
    if (!s) return
    if (s.activeSeconds * 1000 < SESSION_MIN_MS && s.pagesTurned === 0) return
    this.#onEnd({ ...s, activeSeconds: Math.round(s.activeSeconds) })
  }
}

/** A session as extensions receive it (times as ISO 8601). */
export const sessionForExtensions = (s: ReadingSession) => ({
  startedAt: new Date(s.startedAt).toISOString(),
  endedAt: new Date(s.endedAt).toISOString(),
  activeSeconds: s.activeSeconds,
  startFraction: s.startFraction,
  endFraction: s.endFraction,
  pagesTurned: s.pagesTurned,
})
