// Hooks for the in-app end-to-end tests (src/spikes/e2e.ts). They exist only when
// the test harness sets `__LINEN_E2E__` before mounting the app.

import type { LocationHistory } from '../lib/reader/history'
import type { MessageQueue } from '../lib/reader/messages'
import type { ReaderEngine, ReaderLocation } from '../reader/engine'

export interface TestHooks {
  reader?: {
    engine: ReaderEngine
    location: () => ReaderLocation | null
    history: LocationHistory
    bookId: string
  }
  messages?: MessageQueue
  /** Set instead of quitting, so the harness can check what was saved (N5). */
  quitRequested?: boolean
  /** Delay before the reader opens its book, to exercise the slow-open line (G8). */
  openDelayMs?: number
}

export const testHooks: TestHooks | null =
  (globalThis as { __LINEN_E2E__?: TestHooks }).__LINEN_E2E__ ?? null
