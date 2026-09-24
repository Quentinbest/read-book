// Wheel and trackpad page turning in Pages mode (plan I1–I5, P§5).
//
//   I1  notched wheel: one notch = one page, 250 ms cooldown
//   I2  trackpad, vertical: distance accumulates; one turn at 80 px, then locked
//       until the gesture and its momentum end
//   I3  trackpad, horizontal: one turn per swipe
//   I4  the axis is locked by the first 12 px of travel; detection uses
//       accumulated distance, not event counts
//   I5  momentum after a turn is ignored
//
// WKWebView wheel events carry no gesture or momentum phase (Spike B), so the
// end of a gesture is inferred: a quiet gap, or deltas that rise again after
// decaying (a new swipe during the momentum tail). The thresholds below are
// PROVISIONAL until Spike B's traces confirm them.

export type Turn = 'next' | 'prev'

export interface WheelSample {
  /** Pixel deltas (deltaMode 0), positive = content moves up / left (scroll forward). */
  dx: number
  dy: number
  /** Event time in ms. */
  t: number
  /** WebKit's legacy wheelDeltaY: multiples of 120 for a notched mouse wheel. */
  wheelDeltaY?: number
}

export const NOTCH_COOLDOWN_MS = 250
export const TURN_DISTANCE_PX = 80
export const AXIS_LOCK_PX = 12
/** PROVISIONAL: a gap this long ends a gesture (trackpad events arrive every ~8–16 ms). */
export const GESTURE_GAP_MS = 120
/** PROVISIONAL: after decaying below this, a delta this many times larger starts a new swipe. */
export const MOMENTUM_FLOOR_PX = 3
export const REBOUND_FACTOR = 2.5

export function isNotchedWheel(s: WheelSample): boolean {
  const w = s.wheelDeltaY ?? 0
  return w !== 0 && Math.abs(w) % 120 === 0 && s.dx === 0
}

export class WheelTurns {
  #lastNotch = -Infinity
  // Trackpad gesture state
  #active = false
  #axis: 'x' | 'y' | null = null
  #travelX = 0
  #travelY = 0
  #acc = 0
  #locked = false
  #lastT = -Infinity
  #lastMag = 0
  #minMagSinceTurn = Infinity

  /** Feed one wheel event; returns a page turn when one should happen. */
  feed(s: WheelSample): Turn | null {
    if (isNotchedWheel(s)) return this.#notch(s)

    const mag = Math.hypot(s.dx, s.dy)
    const gap = s.t - this.#lastT
    const rebound =
      this.#locked &&
      this.#minMagSinceTurn <= MOMENTUM_FLOOR_PX &&
      mag >= this.#minMagSinceTurn * REBOUND_FACTOR &&
      mag > MOMENTUM_FLOOR_PX * 2
    if (!this.#active || gap > GESTURE_GAP_MS || rebound) this.#start()
    this.#lastT = s.t
    this.#lastMag = mag

    if (this.#locked) {
      this.#minMagSinceTurn = Math.min(this.#minMagSinceTurn, mag)
      return null
    }

    // I4: decide the axis from the first 12 px of travel.
    this.#travelX += Math.abs(s.dx)
    this.#travelY += Math.abs(s.dy)
    if (!this.#axis) {
      if (Math.max(this.#travelX, this.#travelY) < AXIS_LOCK_PX) return null
      this.#axis = this.#travelX > this.#travelY ? 'x' : 'y'
    }
    this.#acc += this.#axis === 'x' ? s.dx : s.dy
    if (Math.abs(this.#acc) >= TURN_DISTANCE_PX) {
      const turn: Turn = this.#acc > 0 ? 'next' : 'prev'
      this.#locked = true
      this.#minMagSinceTurn = mag
      return turn
    }
    return null
  }

  #start() {
    this.#active = true
    this.#axis = null
    this.#travelX = 0
    this.#travelY = 0
    this.#acc = 0
    this.#locked = false
    this.#minMagSinceTurn = Infinity
  }

  #notch(s: WheelSample): Turn | null {
    this.#active = false
    if (s.t - this.#lastNotch < NOTCH_COOLDOWN_MS) return null
    this.#lastNotch = s.t
    // wheelDeltaY is negative when scrolling down (forward).
    return (s.wheelDeltaY ?? 0) < 0 ? 'next' : 'prev'
  }

  get lastMagnitude() {
    return this.#lastMag
  }
}
