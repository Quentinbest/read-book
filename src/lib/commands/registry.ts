// Command registry (plan §3 "Commands", K1–K15).
//
// One registry is the source for ⌘K, the macOS menu bar, the `?` cheat sheet and
// shortcut hints. Features register a handler for a command id when they are
// built; commands without a handler are hidden everywhere.

import { type Chord, type KeyContext, matchesChord, singleKeyAllowed } from './keys'

export type CommandSection = 'reading' | 'navigation' | 'search' | 'annotation' | 'view' | 'app'

export interface CommandDef {
  id: string
  /** Rule ID from the plan, for traceability. */
  rule: string
  title: string
  section: CommandSection
  /** Modified shortcut (macOS). */
  chord?: Chord
  /** Additional chords, e.g. ⌘= for ⌘+. */
  altChords?: Chord[]
  /** Single-key shortcut, gated by `singleKeyAllowed` (K rules). */
  singleKey?: Chord
  /** Shown in ⌘K. Some commands (Esc) are key-only. */
  palette?: boolean
  /** Menu bar placement. */
  menu?: 'File' | 'Edit' | 'View' | 'Go' | 'Window'
}

export interface Command extends CommandDef {
  run(): void
  enabled?(): boolean
}

/** The core commands and their shortcuts (plan §2.8). Handlers are attached by features. */
export const CORE_COMMANDS: CommandDef[] = [
  {
    id: 'chapter.next',
    rule: 'K1',
    title: 'Next chapter',
    section: 'navigation',
    chord: { code: 'ArrowDown', alt: true },
    singleKey: { code: 'BracketRight' },
    palette: true,
    menu: 'Go',
  },
  {
    id: 'chapter.previous',
    rule: 'K1',
    title: 'Previous chapter',
    section: 'navigation',
    chord: { code: 'ArrowUp', alt: true },
    singleKey: { code: 'BracketLeft' },
    palette: true,
    menu: 'Go',
  },
  {
    id: 'search.open',
    rule: 'K2',
    title: 'Search in book',
    section: 'search',
    chord: { code: 'KeyF', meta: true },
    palette: true,
    menu: 'Edit',
  },
  {
    id: 'search.next',
    rule: 'K2',
    title: 'Next result',
    section: 'search',
    chord: { code: 'KeyG', meta: true },
    menu: 'Edit',
  },
  {
    id: 'search.previous',
    rule: 'K2',
    title: 'Previous result',
    section: 'search',
    chord: { code: 'KeyG', meta: true, shift: true },
    menu: 'Edit',
  },
  {
    id: 'navigator.contents',
    rule: 'K3',
    title: 'Go to chapter…',
    section: 'navigation',
    chord: { code: 'KeyT', meta: true },
    palette: true,
    menu: 'Go',
  },
  {
    id: 'goto.open',
    rule: 'K4',
    title: 'Go to location…',
    section: 'navigation',
    chord: { code: 'KeyJ', meta: true },
    palette: true,
    menu: 'Go',
  },
  {
    id: 'navigator.notes',
    rule: 'K5',
    title: 'Highlights and notes',
    section: 'annotation',
    chord: { code: 'KeyA', meta: true, shift: true },
    palette: true,
    menu: 'View',
  },
  {
    id: 'selection.highlight',
    rule: 'K6',
    title: 'Highlight selection',
    section: 'annotation',
    chord: { code: 'KeyH', meta: true, shift: true },
    singleKey: { code: 'KeyH' },
    menu: 'Edit',
  },
  {
    id: 'selection.note',
    rule: 'K6',
    title: 'Add note to selection',
    section: 'annotation',
    chord: { code: 'KeyN', meta: true, shift: true },
    singleKey: { code: 'KeyN' },
    menu: 'Edit',
  },
  {
    id: 'selection.focusBar',
    rule: 'K7',
    title: 'Move to selection actions',
    section: 'annotation',
    chord: { code: 'F6' },
  },
  {
    id: 'reader.caretBrowsing',
    rule: 'K7',
    title: 'Caret browsing',
    section: 'reading',
    chord: { code: 'F7' },
    palette: true,
    menu: 'View',
  },
  {
    id: 'text.larger',
    rule: 'K8',
    title: 'Larger text',
    section: 'view',
    chord: { code: 'Equal', meta: true },
    altChords: [{ code: 'Equal', meta: true, shift: true }],
    palette: true,
    menu: 'View',
  },
  {
    id: 'text.smaller',
    rule: 'K8',
    title: 'Smaller text',
    section: 'view',
    chord: { code: 'Minus', meta: true },
    palette: true,
    menu: 'View',
  },
  {
    id: 'text.reset',
    rule: 'K8',
    title: 'Default text size',
    section: 'view',
    chord: { code: 'Digit0', meta: true },
    palette: true,
    menu: 'View',
  },
  {
    id: 'palette.open',
    rule: 'K9',
    title: 'Command palette',
    section: 'app',
    chord: { code: 'KeyK', meta: true },
    menu: 'View',
  },
  {
    id: 'shortcuts.show',
    rule: 'K9',
    title: 'Keyboard shortcuts',
    section: 'app',
    singleKey: { code: 'Slash', shift: true },
    palette: true,
    menu: 'Window',
  },
  {
    id: 'history.back',
    rule: 'K10',
    title: 'Back',
    section: 'navigation',
    chord: { code: 'BracketLeft', meta: true },
    palette: true,
    menu: 'Go',
  },
  {
    id: 'library.show',
    rule: 'K11',
    title: 'Library',
    section: 'app',
    chord: { code: 'KeyL', meta: true },
    palette: true,
    menu: 'Window',
  },
  {
    id: 'book.open',
    rule: 'K11',
    title: 'Open…',
    section: 'app',
    chord: { code: 'KeyO', meta: true },
    palette: true,
    menu: 'File',
  },
  {
    id: 'window.fullScreen',
    rule: 'K12',
    title: 'Full screen',
    section: 'view',
    chord: { code: 'KeyF', meta: true, ctrl: true },
    palette: true,
    menu: 'View',
  },
  {
    id: 'edit.undo',
    rule: 'K13',
    title: 'Undo',
    section: 'annotation',
    chord: { code: 'KeyZ', meta: true },
    menu: 'Edit',
  },
  {
    id: 'layer.close',
    rule: 'K14',
    title: 'Close layer',
    section: 'reading',
    chord: { code: 'Escape' },
  },
]

export class CommandRegistry {
  #defs = new Map<string, CommandDef>()
  #handlers = new Map<string, Pick<Command, 'run' | 'enabled'>>()
  #listeners = new Set<() => void>()

  constructor(defs: CommandDef[] = CORE_COMMANDS) {
    for (const d of defs) this.define(d)
  }

  define(def: CommandDef) {
    if (this.#defs.has(def.id)) throw new Error(`duplicate command ${def.id}`)
    this.#defs.set(def.id, def)
    this.#emit()
  }

  /**
   * Extension commands: never with a shortcut in the MVP (C4), never replacing
   * a core command.
   */
  defineExtension(extensionId: string, def: Pick<CommandDef, 'title'> & { id: string }) {
    const id = `extension:${extensionId}:${def.id}`
    this.define({ id, rule: 'P1', title: def.title, section: 'app', palette: true })
    return id
  }

  /** Attach a feature's handler. Returns a function that detaches it. */
  handle(id: string, handler: Pick<Command, 'run' | 'enabled'>): () => void {
    if (!this.#defs.has(id)) throw new Error(`unknown command ${id}`)
    this.#handlers.set(id, handler)
    this.#emit()
    return () => {
      this.#handlers.delete(id)
      this.#emit()
    }
  }

  /** Commands that have a handler: the only ones shown anywhere. */
  available(): Command[] {
    const out: Command[] = []
    for (const [id, def] of this.#defs) {
      const h = this.#handlers.get(id)
      if (h) out.push({ ...def, ...h })
    }
    return out
  }

  get(id: string): Command | null {
    const def = this.#defs.get(id)
    const h = this.#handlers.get(id)
    return def && h ? { ...def, ...h } : null
  }

  run(id: string): boolean {
    const c = this.get(id)
    if (!c || c.enabled?.() === false) return false
    c.run()
    return true
  }

  /** Find the command for a key event under the single-key gating rules. */
  commandForKey(
    event: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>,
    ctx: KeyContext,
  ): Command | null {
    for (const c of this.available()) {
      if (c.chord && matchesChord(event, c.chord)) return c
      if (c.altChords?.some((ch) => matchesChord(event, ch))) return c
      if (c.singleKey && matchesChord(event, c.singleKey) && singleKeyAllowed(ctx)) return c
    }
    return null
  }

  subscribe(listener: () => void) {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  #emit() {
    for (const l of this.#listeners) l()
  }
}
