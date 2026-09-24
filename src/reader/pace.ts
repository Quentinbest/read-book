// Reading pace and “N min left in chapter” (plan P§6 location line; decision B2).
//
// Pace is the rolling median words-per-minute over the last 30 minutes of page
// dwell times; a dwell counts only between 2 s and 2 min. It starts at 250 wpm,
// and nothing is shown until 5 minutes have been read or when under a minute is left.

export const DEFAULT_WPM = 250
export const MIN_DWELL_MS = 2_000
export const MAX_DWELL_MS = 120_000
export const WINDOW_MS = 30 * 60_000
export const MIN_READ_MS = 5 * 60_000
/** Characters per word for estimating words from text length. */
export const CHARS_PER_WORD = 6

interface Dwell {
  ms: number
  words: number
  at: number
}

export class ReadingPace {
  #dwells: Dwell[] = []
  #wpm: number
  #readMs: number

  constructor(saved?: { wpm: number; readMs: number }) {
    this.#wpm = saved?.wpm ?? DEFAULT_WPM
    this.#readMs = saved?.readMs ?? 0
  }

  get wpm(): number {
    return this.#wpm
  }

  get readMs(): number {
    return this.#readMs
  }

  /** Record one page read: how long it was on screen and how many characters it held. */
  record(ms: number, chars: number, now: number) {
    if (ms < MIN_DWELL_MS || ms > MAX_DWELL_MS || chars <= 0) return
    this.#readMs += ms
    this.#dwells.push({ ms, words: chars / CHARS_PER_WORD, at: now })
    this.#dwells = this.#dwells.filter((d) => now - d.at <= WINDOW_MS)
    const rates = this.#dwells.map((d) => d.words / (d.ms / 60_000)).sort((a, b) => a - b)
    this.#wpm = rates[Math.floor(rates.length / 2)]
  }

  /** Whole minutes left, or null when nothing should be shown (B2). */
  minutesLeft(charsLeft: number): number | null {
    if (this.#readMs < MIN_READ_MS) return null
    const minutes = charsLeft / CHARS_PER_WORD / this.#wpm
    return minutes < 1 ? null : Math.round(minutes)
  }

  toJSON() {
    return { wpm: Math.round(this.#wpm), readMs: this.#readMs }
  }
}
