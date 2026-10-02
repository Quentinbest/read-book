// Reader lane state machine (plan S1–S8; design S2 and P§21).
//
// Three independent lanes, each holding at most one state:
//   chrome:   immersive | controls
//   docked:   none | Navigator (a tab)          — layout, from 1100 px
//   floating: none | one layer over the text    — Aa, Go to, selection bar,
//             note card, footnote peek, dictionary peek, image view, ⌘K, dialogs, and the
//             Navigator itself below 1100 px
//
// `reduce` is pure: it returns the next state and the effects the UI must run
// (save a note card, return focus to the text). Nothing here touches the DOM.

export const NAVIGATOR_DOCK_MIN_WIDTH = 1100

export type NavigatorTab = 'contents' | 'search' | 'notes' | `extension:${string}`

export type FloatingKind =
  | 'aa'
  | 'goto'
  | 'more'
  | 'selection'
  | 'note'
  | 'peek'
  | 'lookup'
  | 'image'
  | 'palette'
  | 'dialog'
  | 'navigator'

export type Floating =
  { kind: Exclude<FloatingKind, 'navigator'> } | { kind: 'navigator'; tab: NavigatorTab }

export interface ReaderState {
  chrome: 'immersive' | 'controls'
  /** The docked Navigator's tab, or null. Only set when the window is wide. */
  docked: NavigatorTab | null
  floating: Floating | null
  width: number
  /** Chrome state before the Navigator opened, restored when it closes (B11, recommended default). */
  chromeBeforeNavigator: 'immersive' | 'controls' | null
  /** A control in the chrome has focus (S10: the chrome never hides then). */
  chromeFocused: boolean
  /**
   * S9: the pointer at this edge revealed the bars. They stay while the pointer is on a bar
   * or in an edge zone and go the moment it leaves; the top bar names the book only.
   * Null: the full controls (Tab, ⌘J, Aa), or none.
   */
  chromePeek: 'top' | 'bottom' | null
}

export type ReaderEvent =
  | { type: 'openFloating'; kind: Exclude<FloatingKind, 'navigator'> }
  | { type: 'closeFloating' }
  | { type: 'openNavigator'; tab: NavigatorTab }
  | { type: 'closeNavigator' }
  | { type: 'escape' }
  | { type: 'pageTurn' }
  | { type: 'selectionStart' }
  /** With an edge, the pointer's dwell there (S9). Without, the full controls. */
  | { type: 'showChrome'; edge?: 'top' | 'bottom' }
  | { type: 'hideChrome' }
  | { type: 'chromeFocus'; focused: boolean }
  | { type: 'resize'; width: number }

export type Effect =
  /** Save the open note card before it closes (S6). */
  | { type: 'saveNote' }
  /** Return focus to the text at the same reading position (S5). */
  | { type: 'focusText' }

export interface Transition {
  state: ReaderState
  effects: Effect[]
}

/** Layers that suspend reader input and trap focus (S8). */
const MODAL: ReadonlySet<FloatingKind> = new Set(['palette', 'dialog'])
/** Layers a new selection dismisses (S7): popovers and peeks. */
const DISMISSED_BY_SELECTION: ReadonlySet<FloatingKind> = new Set([
  'aa',
  'goto',
  'more',
  'peek',
  'image',
])
/** Popovers anchored to the chrome: the chrome never hides while one is open (S10). */
const CHROME_POPOVERS: ReadonlySet<FloatingKind> = new Set(['aa', 'goto', 'more'])

export function initialState(width: number, dockedTab: NavigatorTab | null = null): ReaderState {
  const wide = width >= NAVIGATOR_DOCK_MIN_WIDTH
  return {
    chrome: 'immersive',
    docked: wide ? dockedTab : null,
    floating: !wide && dockedTab ? { kind: 'navigator', tab: dockedTab } : null,
    width,
    chromeBeforeNavigator: dockedTab ? 'immersive' : null,
    chromeFocused: false,
    chromePeek: null,
  }
}

export const isWide = (s: ReaderState) => s.width >= NAVIGATOR_DOCK_MIN_WIDTH
export const isModal = (s: ReaderState) => s.floating !== null && MODAL.has(s.floating.kind)
/** Reader input (page turns, single-key shortcuts) is suspended only by modals (S8). */
export const readerInputSuspended = isModal
export const navigatorTab = (s: ReaderState): NavigatorTab | null =>
  s.docked ?? (s.floating?.kind === 'navigator' ? s.floating.tab : null)

/** Close the floating layer, saving a note card first. */
function closeFloating(s: ReaderState, effects: Effect[]): ReaderState {
  if (!s.floating) return s
  if (s.floating.kind === 'note') effects.push({ type: 'saveNote' })
  if (s.floating.kind === 'navigator') return closeNavigatorState({ ...s, floating: null })
  return { ...s, floating: null }
}

function closeNavigatorState(s: ReaderState): ReaderState {
  const floating = s.floating?.kind === 'navigator' ? null : s.floating
  return {
    ...s,
    docked: null,
    floating,
    chrome: s.chromeBeforeNavigator ?? s.chrome,
    chromeBeforeNavigator: null,
  }
}

function canHideChrome(s: ReaderState) {
  return !s.chromeFocused && !(s.floating && CHROME_POPOVERS.has(s.floating.kind))
}

export function reduce(state: ReaderState, event: ReaderEvent): Transition {
  const effects: Effect[] = []
  let s = state
  switch (event.type) {
    case 'openFloating': {
      // Opening a floating layer closes the current one (S2).
      if (s.floating?.kind === event.kind) break
      s = closeFloating(s, effects)
      s = { ...s, floating: { kind: event.kind } }
      break
    }
    case 'closeFloating': {
      if (!s.floating) break
      s = closeFloating(s, effects)
      effects.push({ type: 'focusText' })
      break
    }
    case 'openNavigator': {
      const open = navigatorTab(s) !== null
      if (open) {
        // Already open: switch tab.
        s = s.docked
          ? { ...s, docked: event.tab }
          : { ...s, floating: { kind: 'navigator', tab: event.tab } }
        break
      }
      // Chrome and Navigator alternate (S3): opening it hides the bars.
      // A bar the pointer revealed is not restored: the pointer has left it.
      const before = s.chromePeek ? 'immersive' : s.chrome
      if (isWide(s)) {
        s = { ...s, docked: event.tab }
      } else {
        s = closeFloating(s, effects)
        s = { ...s, floating: { kind: 'navigator', tab: event.tab } }
      }
      s = { ...s, chrome: 'immersive', chromeBeforeNavigator: before }
      break
    }
    case 'closeNavigator': {
      if (navigatorTab(s) === null) break
      s = closeNavigatorState(s)
      effects.push({ type: 'focusText' })
      break
    }
    case 'escape': {
      // Esc pops exactly one level: floating → Navigator → chrome. Never leaves the book (S4).
      if (s.floating) {
        s = closeFloating(s, effects)
        effects.push({ type: 'focusText' })
      } else if (s.docked) {
        s = closeNavigatorState(s)
        effects.push({ type: 'focusText' })
      } else if (s.chrome === 'controls') {
        s = { ...s, chrome: 'immersive', chromeFocused: false }
        effects.push({ type: 'focusText' })
      }
      break
    }
    case 'pageTurn': {
      // A page turn closes the floating layer (note saves first) and hides the
      // chrome; the Navigator stays (S6). Modals suspend page turns (S8).
      if (isModal(s)) break
      if (s.floating && s.floating.kind !== 'navigator') s = closeFloating(s, effects)
      s = { ...s, chrome: 'immersive', chromeFocused: false }
      break
    }
    case 'selectionStart': {
      if (s.floating && DISMISSED_BY_SELECTION.has(s.floating.kind)) s = { ...s, floating: null }
      break
    }
    case 'showChrome': {
      // The chrome and the Navigator alternate (S3); the Navigator header stands in for it.
      if (navigatorTab(s) !== null) break
      // S9: an edge never turns the full controls into an edge reveal.
      if (!event.edge) s = { ...s, chrome: 'controls', chromePeek: null }
      else if (s.chrome === 'immersive' || s.chromePeek)
        s = { ...s, chrome: 'controls', chromePeek: event.edge }
      break
    }
    case 'hideChrome': {
      if (s.chrome === 'controls' && canHideChrome(s)) s = { ...s, chrome: 'immersive' }
      break
    }
    case 'chromeFocus': {
      s = { ...s, chromeFocused: event.focused }
      break
    }
    case 'resize': {
      const wasWide = isWide(s)
      s = { ...s, width: event.width }
      const wide = isWide(s)
      if (wasWide && !wide && s.docked) {
        // Below 1100 px the Navigator floats and joins the floating lane (S2).
        // A modal keeps its place; any other floating layer gives way.
        const tab = s.docked
        if (isModal(s)) s = { ...s, docked: null, chromeBeforeNavigator: null }
        else {
          s = closeFloating(s, effects)
          s = { ...s, docked: null, floating: { kind: 'navigator', tab } }
        }
      } else if (!wasWide && wide && s.floating?.kind === 'navigator') {
        s = { ...s, docked: s.floating.tab, floating: null }
      }
      break
    }
  }
  if (s.chrome === 'immersive' && s.chromePeek) s = { ...s, chromePeek: null }
  return { state: s, effects }
}
