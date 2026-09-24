// Page turning from native scroll events (plan I1–I5; Spike B fallback).
//
// AppKit tells us what the WebView cannot: whether the device is precise
// (trackpad, Magic Mouse) or a line-based wheel, and the gesture and momentum
// phases. See src-tauri/src/native_input.rs.

import type { Turn } from './wheel'

export interface NativeScroll {
  precise: boolean
  /** NSEventPhase bits: 1 began, 2 stationary, 4 changed, 8 ended, 16 cancelled, 32 may begin. */
  phase: number
  momentum: number
  /** AppKit scrollingDelta: positive when the content should move down / right. */
  dx: number
  dy: number
  x: number
  y: number
  /** ms */
  t: number
}

export const PHASE = {
  began: 1,
  stationary: 2,
  changed: 4,
  ended: 8,
  cancelled: 16,
  mayBegin: 32,
} as const

/** I1: a pause this long ends a wheel roll; the next wheel event starts a new one. */
export const ROLL_GAP_MS = 150
/** I1: during one continuous roll, at most one turn per this interval. */
export const WHEEL_COOLDOWN_MS = 250
export const TURN_DISTANCE_PX = 80
export const AXIS_LOCK_PX = 12

/**
 * Forward (next page) is content moving up or left, which AppKit reports as a
 * negative scrolling delta. The sign already includes the natural-scrolling
 * setting, so it matches what the reader sees.
 */
const forward = (delta: number): Turn => (delta < 0 ? 'next' : 'prev')

export class NativeTurns {
  // wheel
  #lastWheelT = -Infinity
  #lastTurnT = -Infinity
  // precise gesture
  #inGesture = false
  #axis: 'x' | 'y' | null = null
  #travelX = 0
  #travelY = 0
  #acc = 0
  #locked = false

  feed(e: NativeScroll): Turn | null {
    return e.precise ? this.#precise(e) : this.#wheel(e)
  }

  #wheel(e: NativeScroll): Turn | null {
    const delta = e.dy !== 0 ? e.dy : e.dx
    if (delta === 0) return null
    const newRoll = e.t - this.#lastWheelT > ROLL_GAP_MS
    this.#lastWheelT = e.t
    if (newRoll || e.t - this.#lastTurnT >= WHEEL_COOLDOWN_MS) {
      this.#lastTurnT = e.t
      return forward(delta)
    }
    return null
  }

  #precise(e: NativeScroll): Turn | null {
    // I5: momentum never turns a page.
    if (e.momentum !== 0) return null
    if (e.phase & (PHASE.began | PHASE.mayBegin)) this.#start()
    if (e.phase & (PHASE.ended | PHASE.cancelled)) {
      this.#inGesture = false
      return null
    }
    // Precise devices without phases (rare) behave as one continuous gesture.
    if (!this.#inGesture) this.#start()
    if (this.#locked) return null
    this.#travelX += Math.abs(e.dx)
    this.#travelY += Math.abs(e.dy)
    if (!this.#axis) {
      if (Math.max(this.#travelX, this.#travelY) < AXIS_LOCK_PX) return null
      this.#axis = this.#travelX > this.#travelY ? 'x' : 'y'
    }
    this.#acc += this.#axis === 'x' ? e.dx : e.dy
    if (Math.abs(this.#acc) >= TURN_DISTANCE_PX) {
      this.#locked = true // I2/I3: one turn per gesture, until it ends
      return forward(this.#acc)
    }
    return null
  }

  #start() {
    this.#inGesture = true
    this.#axis = null
    this.#travelX = 0
    this.#travelY = 0
    this.#acc = 0
    this.#locked = false
  }
}
