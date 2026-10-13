// SB2 (docs/selection-bar-plan.md): when the selection bar's tooltip shows. After the
// pointer rests on a button; at once on the next button while the pointer stays in the
// bar, and for a moment after it leaves; at once on keyboard focus. A click, Esc, or the
// bar moving or closing hides it at once. Timing only, so it is tested with fake timers.

export const TOOLTIP_REST_MS = 500
export const TOOLTIP_WARM_MS = 300

export interface Clock {
  now(): number
  set(fn: () => void, ms: number): number
  clear(id: number | undefined): void
}

const windowClock: Clock = {
  now: () => performance.now(),
  set: (fn, ms) => window.setTimeout(fn, ms),
  clear: (id) => window.clearTimeout(id),
}

export interface TooltipTiming<T> {
  /** The button whose tooltip shows. */
  readonly current: T | null
  /** The pointer is over `target`. */
  enter(target: T): void
  /** The pointer left the button it was over. */
  leave(): void
  /** Keyboard focus reached `target`. */
  focus(target: T): void
  /** A click, Esc, or the bar moving or closing: gone at once, and no longer warm. */
  hide(): void
}

export function tooltipTiming<T>(
  onchange: (target: T | null) => void,
  clock: Clock = windowClock,
): TooltipTiming<T> {
  let current: T | null = null
  let pending: number | undefined
  let warmUntil = -Infinity
  const show = (target: T | null) => {
    clock.clear(pending)
    pending = undefined
    if (target === current) return
    current = target
    onchange(target)
  }
  return {
    get current() {
      return current
    },
    enter(target) {
      if (current !== null || clock.now() < warmUntil) return show(target)
      clock.clear(pending)
      pending = clock.set(() => show(target), TOOLTIP_REST_MS)
    },
    leave() {
      if (current !== null) warmUntil = clock.now() + TOOLTIP_WARM_MS
      show(null)
    },
    focus(target) {
      show(target)
    },
    hide() {
      warmUntil = -Infinity
      show(null)
    },
  }
}
