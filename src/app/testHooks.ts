// Hooks for the in-app end-to-end tests (src/spikes/e2e.ts). They exist only when
// the test harness sets `__LINEN_E2E__` before mounting the app.

import type { LocationHistory } from '../lib/reader/history'
import type { MessageQueue } from '../lib/reader/messages'
import type { CommandRegistry } from '../lib/commands/registry'
import type { ReaderEngine, ReaderLocation } from '../reader/engine'

export interface TestHooks {
  reader?: {
    engine: ReaderEngine
    location: () => ReaderLocation | null
    history: LocationHistory
    bookId: string
    /** B1: whether every section's pages have been counted at this layout. */
    pagesExact: () => boolean
    /** F1–F8: the reader's search state. */
    search: import('../reader/search.svelte').SearchState
    /** A4–A9: the book's highlights and notes, and what ⌘Z can undo. */
    annotations: import('../reader/annotations.svelte').Annotations
    undo: import('../reader/annotations.svelte').UndoStack
  }
  messages?: MessageQueue
  registry?: CommandRegistry
  writes?: import('./writes').WriteQueue
  /** Run a command as the menu or ⌘K would. */
  run?: (id: string) => boolean
  /** Set instead of quitting, so the harness can check what was saved (N5). */
  quitRequested?: boolean
  /** N5: what the quit handler would answer (everything saved or not). */
  quitSaved?: boolean
  /** External links the reader would have opened; tests never open the system browser (N10). */
  externalOpened?: string[]
  /** A10: the native context menu's items, recorded instead of shown (menus block the harness). */
  contextMenu?: { labels: string[]; run: (label: string) => void }
  /** Phase 7: the path the install dialog would return (the harness cannot drive it). */
  pickExtensionFile?: () => Promise<string | null>
  /** Phase 7: where the save dialog would save (files.save). */
  pickSavePath?: (suggested: string) => Promise<string | null>
  /** Phase 7: the extension host. */
  extensions?: import('../extensions/host.svelte').ExtensionHost
  /** G2: times Settings… asked for the Settings window (not opened under test). */
  settingsOpened?: number
  /** B8: relayout this many ms after the book starts opening (a race the restore must survive). */
  relayoutDuringOpenMs?: number[]
  /** S14: let the book go on leaving it, so every open is cold (the default under test). */
  noWarm?: boolean
  /** V8: cover-grow transitions started (library → book). */
  coverGrows?: number
  /** D7-WebKit: act as if this WebKit were older than Safari 16.4. */
  webkitTooOld?: boolean
  /** Delay before the reader opens its book, to exercise the slow-open line (G8). */
  openDelayMs?: number
}

export const testHooks: TestHooks | null =
  (globalThis as { __LINEN_E2E__?: TestHooks }).__LINEN_E2E__ ?? null
