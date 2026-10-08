// Key chords bound to physical key positions (plan T7: KeyboardEvent.code), the
// single-key gating rules (§2.8), and macOS shortcut labels.

import { t } from '../strings'

export interface Chord {
  /** KeyboardEvent.code: the physical key, independent of keyboard layout. */
  code: string
  meta?: boolean
  ctrl?: boolean
  alt?: boolean
  shift?: boolean
}

export interface KeyContext {
  /** Focus is on the page (the book text), not on a control. */
  pageFocused: boolean
  /** An input, textarea or contenteditable has focus. */
  textFieldActive: boolean
  /** The user setting for single-key shortcuts (on by default). */
  singleKeysEnabled: boolean
  /** A screen reader is running (T6); single-key shortcuts switch off. */
  screenReaderRunning: boolean
  /** A modal (⌘K, a dialog) is open and suspends reader input (S8). */
  modalOpen: boolean
}

export function matchesChord(
  e: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>,
  c: Chord,
): boolean {
  return (
    e.code === c.code &&
    e.metaKey === !!c.meta &&
    e.ctrlKey === !!c.ctrl &&
    e.altKey === !!c.alt &&
    e.shiftKey === !!c.shift
  )
}

/** Single-key shortcuts (H, N, [, ], ?) act only when all of these hold (§2.8). */
export function singleKeyAllowed(ctx: KeyContext): boolean {
  return (
    ctx.pageFocused &&
    !ctx.textFieldActive &&
    ctx.singleKeysEnabled &&
    !ctx.screenReaderRunning &&
    !ctx.modalOpen
  )
}

/**
 * Default key-cap labels for a US layout. On macOS the real labels come from a
 * native layout query (T7) and override these; the binding itself never changes.
 */
const US_LABELS: Record<string, string> = {
  ArrowDown: '↓',
  ArrowUp: '↑',
  ArrowLeft: '←',
  ArrowRight: '→',
  BracketLeft: '[',
  BracketRight: ']',
  Equal: '+',
  Minus: '−',
  Slash: '/',
  Enter: '↵',
  Backspace: '⌫',
  Tab: '⇥',
  Comma: ',',
  Period: '.',
}

/** Keys whose labels are symbolic in the design (⌘+ ⌘−), whatever the layout prints. */
const SYMBOLIC = new Set(['Equal', 'Minus'])

export function keyLabel(code: string, layout?: ReadonlyMap<string, string>): string {
  const native = SYMBOLIC.has(code) ? undefined : layout?.get(code)
  if (native) return native.length === 1 ? native.toUpperCase() : native
  if (code === 'Escape') return t.keys.esc
  if (code === 'Space') return t.keys.space
  if (code in US_LABELS) return US_LABELS[code]
  const m = /^(Key|Digit)(.)$/.exec(code)
  if (m) return m[2]
  return code // F6, F7, …
}

/** macOS label in the platform order ⌃⌥⇧⌘, e.g. ⇧⌘G. `?` is shown as itself. */
export function chordLabel(c: Chord, layout?: ReadonlyMap<string, string>): string {
  if (c.code === 'Slash' && c.shift && !c.meta && !c.ctrl && !c.alt) return '?'
  return (
    (c.ctrl ? '⌃' : '') +
    (c.alt ? '⌥' : '') +
    (c.shift ? '⇧' : '') +
    (c.meta ? '⌘' : '') +
    keyLabel(c.code, layout)
  )
}

/** True when the focused element takes text input. */
export function isTextField(el: Element | null): boolean {
  if (!el) return false
  if (el instanceof HTMLTextAreaElement) return !el.readOnly
  if (el instanceof HTMLInputElement) {
    return (
      ![
        'button',
        'checkbox',
        'radio',
        'range',
        'color',
        'file',
        'submit',
        'reset',
        'image',
      ].includes(el.type) && !el.readOnly
    )
  }
  return (el as HTMLElement).isContentEditable === true
}
