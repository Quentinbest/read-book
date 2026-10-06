// ReaderEngine: the adapter around foliate-js (plan §3; decisions D-E1, D-D1, D-X1).
//
// foliate-js has no stable API, so everything the app needs from it goes
// through here: opening, layout, page turns, locations, links and the content
// hooks. Spike findings built in:
// - content passes through `transformContent` (per-document CSP, CSS sanitiser);
// - every foliate link event is routed by the app (N10);
// - one column below the spread breakpoint (L8);
// - page turns queue at most one pending turn (I6) instead of being dropped;
// - D-D1: the previous and next units are laid out ahead in hidden views, so a
//   turn across a boundary is a swap, not a load;
// - L16: chapters over ~1 MB are laid out one chunk at a time (see chunks.ts); a
//   unit is (section, chunk), and chunk boundaries behave like section ones;
// - scrolls the engine did not make (VoiceOver, X3) are detected, snapped to a
//   whole page and reported as a location change;
// - Scroll mode (B8, G8): the same three views are stacked in the host, which
//   becomes a native scroller; each shows one unit at its full height, and the
//   stack slides forward and back as the reader scrolls between chapters.

import 'foliate-js/view.js'
import { compare as compareCfi } from 'foliate-js/epubcfi.js'
import type { Book, View } from 'foliate-js/view.js'
import {
  CHUNK_THRESHOLD_BYTES,
  chunkCount,
  chunkOf,
  computeChunks,
  sectionFraction,
  showChunk,
  type Chunks,
} from './chunks'
import { transformContent } from './content'
import type { PublisherStyles } from './typography'
import { isNoteRef, noteContainer, referencedFootnoteAsides } from './notes'
import { extractText, offsetAt, rangeFor, type ExtractedText } from '../lib/search/extract'
import { MIN_SIDE_MARGIN, type Layout } from './layout'
import type { EntryLoader } from './loader'
import { PARAGRAPH_SPACING_CSS } from './styles'
import { locate, quoteAt, type Placement, type TextQuote } from '../lib/annotations/anchor'

/** Reading order of two CFIs (A8): negative, zero or positive. */
export function cfiOrder(a: string, b: string): number {
  try {
    return compareCfi(a, b)
  } catch {
    return a < b ? -1 : a > b ? 1 : 0
  }
}

export interface ReaderLocation {
  cfi: string
  /** Position in the whole book, 0–1. */
  fraction: number
  sectionIndex: number
  sectionCount: number
  /** Label of the current table-of-contents entry. */
  chapterLabel: string
  /** Its href, to mark “You are here” in Contents (N6). */
  tocHref?: string
  /** Characters left in this section (B2 turns them into minutes). */
  sectionCharsLeft: number
  /** Page within the section (1-based) and the section's page count, when laid out. */
  page?: number
  pages?: number
  /** Fixed layout (G8): the book pages on screen, 1-based (two in a spread). */
  fixedPages?: number[]
  /** Page numbers are estimates (a chunked chapter, L16): show “≈”. */
  approximate: boolean
  /** Place within the section, 0–1; set when `approximate` (page/pages then count the laid-out chunk only). */
  sectionFraction?: number
  reason: 'page' | 'navigation' | 'scroll' | 'selection' | 'anchor' | 'snap' | 'external'
}

/** N9: a note reference was followed; the app shows a peek instead of navigating. */
export interface NoteEvent {
  marker: Element
  href: string
  /** The marker's text, e.g. “1”. */
  label: string
  /** The note's container in its document, or null when it cannot be found. */
  note: Element | null
  /** The marker's box in window coordinates. */
  rect: DOMRect
}

/** F5: search marks: every match per section, the active one, and the theme's colours. */
export interface SearchMarks {
  bySection: Map<number, { start: number; end: number }[]>
  active: { index: number; start: number; end: number } | null
  colors: { tint: string; outline: string; activeTint: string; activeOutline: string }
}

interface MarkLayer {
  add(
    key: string,
    range: Range,
    draw: (rects: DOMRectList, o: MarkStyle) => SVGElement,
    o: MarkStyle,
  ): void
  remove(key: string): void
}
interface MarkStyle {
  fill: string
  stroke: string
  width: number
}

/** F5: a soft tint with a 1 px outline; the active match a stronger tint and a 2 px accent outline. */
function drawMark(rects: DOMRectList, o: MarkStyle): SVGElement {
  const ns = 'http://www.w3.org/2000/svg'
  const g = document.createElementNS(ns, 'g')
  for (const r of Array.from(rects)) {
    const el = document.createElementNS(ns, 'rect')
    const inset = o.width / 2
    el.setAttribute('x', String(r.left - inset))
    el.setAttribute('y', String(r.top - inset))
    el.setAttribute('width', String(r.width + o.width))
    el.setAttribute('height', String(r.height + o.width))
    el.setAttribute('rx', '2')
    el.setAttribute('fill', o.fill)
    el.setAttribute('stroke', o.stroke)
    el.setAttribute('stroke-width', String(o.width))
    g.append(el)
  }
  return g
}

/** A1: text selected in the book (one chapter; B4). Offsets are in the chapter's extracted text. */
export interface SelectionEvent {
  index: number
  range: Range
  cfi: string
  start: number
  end: number
  text: string
  /** A9: the text quote kept beside the CFI. */
  quote: TextQuote
  /** The first and last selected lines, in window coordinates (A1 placement). */
  first: DOMRect
  last: DOMRect
}

/** A4: a highlight to draw: tint plus a 2 px underline; `note` adds the margin dot (A6). */
export interface HighlightMark {
  id: string
  cfi: string
  color: 'yellow' | 'green' | 'blue' | 'rose'
  note: boolean
}
export type HighlightColors = Record<HighlightMark['color'], { tint: string; underline: string }>
/**
 * A4: highlight colours, and for WebKit without custom highlights the overlay
 * tints and whether they multiply with the page (light themes).
 */
export interface HighlightStyle {
  colors: HighlightColors
  overlayTints: Record<HighlightMark['color'], string>
  multiply: boolean
}

/** The line boxes of a range's text only (element boxes would tint whole paragraphs). */
function textRects(range: Range): DOMRect[] {
  const doc = range.startContainer.ownerDocument
  if (!doc) return []
  const root = range.commonAncestorContainer
  const out: DOMRect[] = []
  const walker = doc.createTreeWalker(root.nodeType === 3 ? root.parentNode! : root, 4)
  const part = doc.createRange()
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!range.intersectsNode(n)) continue
    const text = n as Text
    part.setStart(text, n === range.startContainer ? range.startOffset : 0)
    part.setEnd(text, n === range.endContainer ? range.endOffset : text.data.length)
    for (const r of Array.from(part.getClientRects())) if (r.width > 0 && r.height > 0) out.push(r)
  }
  return out
}

/** The edge of the text column beside a line (for the note dot, A6; Screen 07). */
function columnEdge(range: Range, line: DOMRect, rtl: boolean): number {
  let el: Element | null =
    range.startContainer.nodeType === 1
      ? (range.startContainer as Element)
      : range.startContainer.parentElement
  const view = range.startContainer.ownerDocument?.defaultView
  while (el && view && view.getComputedStyle(el).display.startsWith('inline')) el = el.parentElement
  const box = Array.from(el?.getClientRects() ?? []).find(
    (r) => r.top <= line.top + 1 && r.bottom >= line.bottom - 1 && r.left <= line.left + 1,
  )
  return rtl ? (box?.left ?? line.left) : (box?.right ?? line.right)
}

/** The document's CSS custom highlight registry, where WebKit has one (17.2+). */
function customHighlights(
  doc: Document,
): { set(name: string, h: unknown): void; delete(name: string): void } | null {
  const win = doc.defaultView as unknown as {
    CSS?: { highlights?: { set(n: string, h: unknown): void; delete(n: string): void } }
    Highlight?: unknown
  } | null
  return win?.CSS?.highlights && win.Highlight ? win.CSS.highlights : null
}

function drawHighlight(
  _rects: DOMRectList,
  o: { tint: string | null; underline: string; dot: boolean; range: Range; multiply: boolean },
): SVGElement {
  const ns = 'http://www.w3.org/2000/svg'
  const g = document.createElementNS(ns, 'g')
  // The overlay lies over the text: on light themes the tint multiplies with the
  // page, so text shows through as ink on the tint; Night's tints are translucent.
  const tints = document.createElementNS(ns, 'g')
  tints.setAttribute('data-linen-tints', '')
  if (o.multiply) tints.style.mixBlendMode = 'multiply'
  g.append(tints)
  const doc = o.range.startContainer.ownerDocument
  const style = doc?.body ? doc.defaultView?.getComputedStyle(doc.body) : null
  const vertical = style?.writingMode.startsWith('vertical') ?? false
  const rtl = style?.direction === 'rtl'
  const list = textRects(o.range)
  const rect = (x: number, y: number, w: number, h: number, fill: string, into = g) => {
    const el = document.createElementNS(ns, 'rect')
    el.setAttribute('x', String(x))
    el.setAttribute('y', String(y))
    el.setAttribute('width', String(w))
    el.setAttribute('height', String(h))
    el.setAttribute('fill', fill)
    into.append(el)
  }
  for (const r of list) {
    if (o.tint) rect(r.left, r.top, r.width, r.height, o.tint, tints)
    // A4: a 2 px underline; under vertical text it runs down the right side.
    if (vertical) rect(r.right - 2, r.top, 2, r.height, o.underline)
    else rect(r.left, r.bottom - 2, r.width, 2, o.underline)
  }
  if (o.dot && list[0] && !vertical) {
    // A6: a dot in the margin beside the passage's first line (Screen 07).
    const edge = columnEdge(o.range, list[0], rtl)
    const dot = document.createElementNS(ns, 'circle')
    dot.setAttribute('cx', String(rtl ? edge - 9.5 : edge + 9.5))
    dot.setAttribute('cy', String(list[0].top + list[0].height / 2))
    dot.setAttribute('r', '3.5')
    dot.setAttribute('fill', o.underline)
    dot.setAttribute('data-linen-note-dot', '')
    g.append(dot)
  }
  return g
}

/** V8: a jump pulses the passage for 1.2 s. */
function drawPulse(_rects: DOMRectList, o: { color: string; range: Range }): SVGElement {
  const ns = 'http://www.w3.org/2000/svg'
  const g = document.createElementNS(ns, 'g')
  for (const r of textRects(o.range)) {
    const el = document.createElementNS(ns, 'rect')
    // Screen 08: a 3 px outline 2 px outside the passage.
    el.setAttribute('x', String(r.left - 3.5))
    el.setAttribute('y', String(r.top - 3.5))
    el.setAttribute('width', String(r.width + 7))
    el.setAttribute('height', String(r.height + 7))
    el.setAttribute('rx', '2')
    el.setAttribute('fill', 'none')
    el.setAttribute('stroke', o.color)
    el.setAttribute('stroke-width', '3')
    g.append(el)
  }
  g.animate([{ opacity: 1 }, { opacity: 1, offset: 0.6 }, { opacity: 0 }], {
    duration: 1200,
    easing: 'ease-out',
    fill: 'forwards',
  })
  return g
}

/** N11: an image in the text was clicked. */
export interface ImageEvent {
  src: string
  alt: string
  caption: string
  element: Element
}

export type Turn = 'next' | 'prev'
export type ReadingMode = 'pages' | 'scroll'

export interface LinkEvent {
  href: string
  external: boolean
}

type Listener<T> = (value: T) => void

/** A place a view can show: a section, and a chunk of it (0 unless chunked; -1 = its last chunk). */
interface Unit {
  index: number
  chunk: number
}

/** A hidden view parked at a neighbouring unit, ready to be swapped in. */
interface Neighbour {
  view: View
  unit: Unit | null
  ready: Promise<void>
}

/** Scroll mode: a view stacked in the scroller at `top`, showing `unit` at full height. */
/** Where a Scroll-mode navigation goes: a CFI or href, a section, a fraction, or a range. */
type ScrollTarget = string | number | { fraction: number } | Range
const isRange = (t: ScrollTarget): t is Range => typeof t === 'object' && 'startContainer' in t

interface Slot {
  view: View
  unit: Unit
  top: number
  height: number
}

/** Which chunk a view should lay out when its next section loads. */
type ChunkRequest =
  | { kind: 'chunk'; chunk: number }
  | { kind: 'last' }
  | { kind: 'node'; node: (doc: Document) => Node | null }
  | { kind: 'fraction'; fraction: number }

const nextFrame = () => new Promise((r) => requestAnimationFrame(r))
/** G8 Scroll mode: the fades under the window edges, and the space between chapters. */
export const SCROLL_FADE_TOP = 40
export const SCROLL_FADE_BOTTOM = 88
const SCROLL_JOIN = 124
/** I9: ↓ ↑ scroll by this many lines. */
export const SCROLL_LINES = 3
/** G8: fixed-layout pages sit inside 64 px margins above and below. */
const FIXED_VERTICAL_MARGIN = 64
/** G8, I17: zoom levels for fixed-layout pages, relative to fit. */
export const ZOOM_LEVELS = [1, 1.5, 2, 3, 4]
/** B1 idle counting: check for a quiet moment this often, and call it quiet after this long. */
const COUNT_STEP_MS = 60
const COUNT_QUIET_MS = 400

export class ReaderEngine {
  #host: HTMLElement
  #book: Book | null = null
  #current: View
  #next: Neighbour
  #prev: Neighbour
  #relocate = new Set<Listener<ReaderLocation>>()
  #link = new Set<Listener<LinkEvent>>()
  #key = new Set<Listener<KeyboardEvent>>()
  #doc = new Set<Listener<Document>>()
  #notes = new Set<Listener<NoteEvent>>()
  #images = new Set<Listener<ImageEvent>>()
  #hover = new Set<Listener<string | null>>()
  #selection = new Set<Listener<SelectionEvent | null>>()
  #highlightClick = new Set<Listener<{ id: string; first: DOMRect; last: DOMRect }>>()
  #highlights: { marks: HighlightMark[]; style: HighlightStyle } | null = null
  #hlKeys = new WeakMap<object, string[]>()
  #hadSelection = false
  #caret: { color: string } | null = null
  #closed = false
  #extraStyles = ''
  #publisher: PublisherStyles = 'balanced'
  /**
   * I16: Scroll for a vertical-writing book runs sideways. It is foliate-js's own
   * scrolled flow (which scrolls vertical text horizontally) in the page box, one
   * chapter at a time; the next or previous chapter loads at either end. Internally
   * the engine stays in Pages (one view and its neighbours).
   */
  #sideways = false
  /** Tests turn this off to exercise the overlay path older WebKit takes (A4). */
  useCustomHighlights = true
  /** Extracted text of live documents, for search offsets (F5). */
  #texts = new WeakMap<Document, ExtractedText>()
  #marks: SearchMarks | null = null
  #markKeys = new WeakMap<object, string[]>()
  /** What each overlay last drew, so an unchanged page is not redrawn. */
  #markDrawn = new WeakMap<object, { list: unknown; active: string; set: SearchMarks | null }>()
  /** What the last mark drawing did, for the end-to-end tests (F5). */
  #markStats = {
    drawn: 0,
    ms: 0,
    active: null as MarkStyle | null,
    soft: null as MarkStyle | null,
  }
  #chunks = new WeakMap<Document, Chunks>()
  /** Section index of every loaded document (fixed-layout contents lack it). */
  #docIndex = new WeakMap<Document, number>()
  #requests = new WeakMap<View, ChunkRequest>()
  #turning: Promise<void> | null = null
  #pending: Turn | null = null
  #styles = ''
  #layout: Layout | null = null
  #lastPage = -1
  #watchTimer = 0
  /** B1: a hidden view that lays out sections in idle time to count their pages. */
  #counter: View | null = null
  #lastInputAt = 0
  #zoom = 1
  #fitScale = 1
  #mode: ReadingMode = 'pages'
  /** Scroll mode: stacked views, top to bottom (at most the three reading views). */
  #slots: Slot[] = []
  #joins: HTMLElement[] = []
  #spacer: HTMLElement | null = null
  #stackBusy = false
  /** The newest stack build; an older one still running gives way to it. */
  #stackSeq = 0
  /**
   * B8: the place a Scroll-mode navigation asked for. Until the reader moves, a
   * relayout or a late change of height returns to it, not to whatever the screen
   * happens to show at that moment (which, mid-build, can be a neighbouring chapter).
   * `y` is the scroll position the engine set; any other position means the reader moved.
   */
  #scrollPin: { target: ScrollTarget; view: View | null; y: number } | null = null
  #scrollFrame = 0
  #heightTimer = 0
  /** Set while the engine reports a scroll location, so foliate's own reports are ignored. */
  #emitting = false
  /** Scroll mode: where the reported place sits within its unit, 0–1 (for chunked chapters). */
  #scrollInUnit = 0
  /** Recent engine events, for diagnosing view swaps. */
  #trail: string[] = []

  constructor(host: HTMLElement) {
    this.#host = host
    this.#current = this.#createView()
    this.#next = { view: this.#createView(), unit: null, ready: Promise.resolve() }
    this.#prev = { view: this.#createView(), unit: null, ready: Promise.resolve() }
    this.#show(this.#current)
    host.addEventListener('scroll', () => {
      if (this.#mode !== 'scroll' || this.#scrollFrame) return
      this.#scrollFrame = requestAnimationFrame(() => {
        this.#scrollFrame = 0
        this.#onScroll()
      })
    })
  }

  /** The visible view. */
  get view(): View {
    return this.#current
  }

  get book(): Book | null {
    return this.#book
  }

  /** A fixed-layout book (E2): one view, pages scaled to fit, no neighbours. */
  get fixedLayout(): boolean {
    return this.#current.isFixedLayout === true
  }

  /** The book's page progression is right to left (I15). */
  get rtl(): boolean {
    return this.#book?.dir === 'rtl'
  }

  onRelocate(l: Listener<ReaderLocation>) {
    this.#relocate.add(l)
    return () => this.#relocate.delete(l)
  }

  /** Keys pressed while focus is inside the book document (they never reach the app window). */
  onKey(l: Listener<KeyboardEvent>) {
    this.#key.add(l)
    return () => this.#key.delete(l)
  }

  /** Every book document as it loads, in any view (for per-document work such as L15). */
  onDocument(l: Listener<Document>) {
    this.#doc.add(l)
    return () => this.#doc.delete(l)
  }

  /** N9: note references open a peek; they never navigate. */
  onNote(l: Listener<NoteEvent>) {
    this.#notes.add(l)
    return () => this.#notes.delete(l)
  }

  /** N11: images open the image view. */
  onImage(l: Listener<ImageEvent>) {
    this.#images.add(l)
    return () => this.#images.delete(l)
  }

  /** N10: the URL of the external link under the pointer, or null when it leaves. */
  onLinkHover(l: Listener<string | null>) {
    this.#hover.add(l)
    return () => this.#hover.delete(l)
  }

  // ---- A11, T3: caret browsing. WebKit has none, so the caret is ours: a
  // collapsed selection in the page, drawn in the overlay; the arrows move it and
  // ⇧ + arrows extend it with Selection.modify (a selection is then reported as
  // usual, A1). Moving past the page turns it.

  get caretBrowsing(): boolean {
    return this.#caret !== null
  }

  /** F7: turn caret browsing on (at the start of the page, if nothing is selected) or off. */
  setCaretBrowsing(color: string | null) {
    this.#caret = color ? { color } : null
    const c = this.#pageContents()
    if (!c) return
    if (color) {
      const sel = c.doc.getSelection()
      if (!sel?.rangeCount || !this.#onPage(sel.getRangeAt(0))) this.#caretToPageStart()
      c.doc.body?.focus?.()
    }
    this.#drawCaret()
  }

  /**
   * Handle an arrow, Home or End key while caret browsing. Returns true when the
   * key was used (the reader must not turn the page for it).
   */
  caretKey(e: KeyboardEvent): boolean {
    if (!this.#caret || e.metaKey || e.ctrlKey) return false
    const c = this.#pageContents()
    const sel = c?.doc.getSelection() as
      | (Selection & {
          modify?: (alter: string, direction: string, granularity: string) => void
        })
      | null
    if (!c || !sel?.modify) return false
    const vertical = this.#vertical()
    const rtl = this.rtl
    const moves: Record<string, [string, string]> = {
      ArrowLeft: vertical
        ? ['forward', 'line']
        : [rtl ? 'forward' : 'backward', e.altKey ? 'word' : 'character'],
      ArrowRight: vertical
        ? ['backward', 'line']
        : [rtl ? 'backward' : 'forward', e.altKey ? 'word' : 'character'],
      ArrowUp: vertical ? ['backward', 'character'] : ['backward', e.altKey ? 'paragraph' : 'line'],
      ArrowDown: vertical ? ['forward', 'character'] : ['forward', e.altKey ? 'paragraph' : 'line'],
      Home: ['backward', 'lineboundary'],
      End: ['forward', 'lineboundary'],
    }
    const move = moves[e.key]
    if (!move) return false
    if (!sel.rangeCount) this.#caretToPageStart()
    sel.modify(e.shiftKey ? 'extend' : 'move', move[0], move[1])
    this.#drawCaret()
    // Keep the caret on screen: past the page's end or start, turn the page.
    const focus = c.doc.createRange()
    if (sel.focusNode) focus.setStart(sel.focusNode, sel.focusOffset)
    if (sel.focusNode && !this.#onPage(focus))
      void this.turn(move[0] === 'forward' ? 'next' : 'prev').then(() => this.#drawCaret())
    return true
  }

  #pageContents(): { doc: Document; index: number; overlayer?: unknown } | null {
    const index = this.location?.sectionIndex
    const c = this.#current.renderer
      ?.getContents()
      .find((x) => x.doc && (index === undefined || x.index === index))
    return c?.doc ? (c as { doc: Document; index: number; overlayer?: unknown }) : null
  }

  /** Whether a range starts on the page shown. */
  #onPage(range: Range): boolean {
    const shown = this.#current.lastLocation?.range
    if (!shown || shown.startContainer.ownerDocument !== range.startContainer.ownerDocument)
      return false
    try {
      return (
        // The page starts at or before it, and ends at or after its start.
        shown.compareBoundaryPoints(Range.START_TO_START, range) <= 0 &&
        shown.compareBoundaryPoints(Range.START_TO_END, range) >= 0
      )
    } catch {
      return false
    }
  }

  #caretToPageStart() {
    const shown = this.#current.lastLocation?.range
    const doc = shown?.startContainer.ownerDocument
    const sel = doc?.getSelection()
    if (!shown || !sel) return
    sel.removeAllRanges()
    sel.collapse(shown.startContainer, shown.startOffset)
  }

  #drawCaret() {
    for (const c of this.#current.renderer?.getContents() ?? []) {
      const layer = c.overlayer as MarkLayer | undefined
      if (!layer || !c.doc) continue
      layer.remove('linen-caret')
      const sel = c.doc.getSelection()
      if (!this.#caret || !sel?.focusNode || !sel.isCollapsed) continue
      const range = c.doc.createRange()
      range.setStart(sel.focusNode, sel.focusOffset)
      const vertical = this.#vertical()
      const color = this.#caret.color
      ;(layer as unknown as { add: (...a: unknown[]) => void }).add(
        'linen-caret',
        range,
        (rects: DOMRectList) => {
          const ns = 'http://www.w3.org/2000/svg'
          const r = rects[0] ?? range.getBoundingClientRect()
          const el = document.createElementNS(ns, 'rect')
          el.setAttribute('x', String(vertical ? r.left : r.left - 1))
          el.setAttribute('y', String(vertical ? r.top - 1 : r.top))
          el.setAttribute('width', String(vertical ? r.width : 2))
          el.setAttribute('height', String(vertical ? 2 : r.height))
          el.setAttribute('fill', color)
          el.setAttribute('data-linen-caret', '')
          return el
        },
        {},
      )
    }
  }

  /** A10: the highlight under a pointer event in the book, if any. */
  highlightAt(e: MouseEvent): string | null {
    const doc = (e.target as Node | null)?.ownerDocument
    for (const c of this.#current.renderer?.getContents() ?? []) {
      if (c.doc !== doc) continue
      const layer = c.overlayer as
        { hitTest?: (p: { x: number; y: number }) => [string?] } | undefined
      const [key] = layer?.hitTest?.({ x: e.clientX, y: e.clientY }) ?? []
      return key?.startsWith('linen-hl:') ? key.slice(9) : null
    }
    return null
  }

  /** A1: the reader selected text (or the selection went: null). */
  onSelection(l: Listener<SelectionEvent | null>) {
    this.#selection.add(l)
    return () => this.#selection.delete(l)
  }

  /** A5: a plain click on a highlight. */
  onHighlightClick(l: Listener<{ id: string; first: DOMRect; last: DOMRect }>) {
    this.#highlightClick.add(l)
    return () => this.#highlightClick.delete(l)
  }

  /** A4, A6: draw highlights (tint, 2 px underline, note dot) in every loaded chapter. */
  setHighlights(marks: HighlightMark[] | null, style?: HighlightStyle) {
    this.#highlights = marks && style ? { marks, style } : null
    for (const view of this.#views()) this.#drawHighlights(view)
  }

  /** Draw the highlights again with what was last set (tests switch drawing paths). */
  redrawHighlights() {
    for (const view of this.#views()) this.#drawHighlights(view)
  }

  /** Clear the selection in the book (after an action, A1). */
  clearSelection() {
    for (const view of this.#views())
      for (const c of view.renderer?.getContents() ?? []) c.doc?.getSelection()?.removeAllRanges()
    this.#hadSelection = false
  }

  /** A range's lines in window coordinates. */
  #screenRects(range: Range): { first: DOMRect; last: DOMRect } | null {
    const rects = Array.from(range.getClientRects()).filter((r) => r.width > 0 && r.height > 0)
    const frame =
      range.startContainer.ownerDocument?.defaultView?.frameElement?.getBoundingClientRect()
    if (!rects.length || !frame) return null
    const move = (r: DOMRect) =>
      new DOMRect(r.left + frame.left, r.top + frame.top, r.width, r.height)
    return { first: move(rects[0]), last: move(rects[rects.length - 1]) }
  }

  /** The Range a CFI points at in a loaded document, if it is loaded. */
  #cfiRange(view: View, cfi: string, doc: Document, index: number): Range | null {
    try {
      const r = view.resolveNavigation(cfi) as
        { index: number; anchor: (d: Document) => Range | Element | null } | undefined
      if (!r || r.index !== index) return null
      const a = r.anchor(doc)
      if (!a) return null
      if ('startContainer' in a) return a
      const range = doc.createRange()
      range.selectNodeContents(a)
      return range
    } catch {
      return null
    }
  }

  #drawHighlights(view: View) {
    for (const c of view.renderer?.getContents() ?? []) {
      const layer = c.overlayer as MarkLayer | undefined
      if (!layer || !c.doc) continue
      for (const key of this.#hlKeys.get(layer) ?? []) layer.remove(key)
      const keys: string[] = []
      const h = this.#highlights
      // A4: the tint goes behind the text, as a CSS custom highlight in the book's
      // document where WebKit has them (Safari 17.2+); older WebKit multiplies an
      // overlay tint with the page instead. Underlines and note dots are overlay drawings.
      const registry = this.useCustomHighlights ? customHighlights(c.doc) : null
      if (!registry)
        for (const color of ['yellow', 'green', 'blue', 'rose'])
          customHighlights(c.doc)?.delete(`linen-hl-${color}`)
      const byColor: Record<string, Range[]> = {}
      for (const m of h?.marks ?? []) {
        const range = this.#cfiRange(view, m.cfi, c.doc, c.index)
        if (!range || !h) continue
        ;(byColor[m.color] ??= []).push(range)
        const key = `linen-hl:${m.id}`
        const colors = h.style.colors[m.color]
        ;(layer as unknown as { add: (...a: unknown[]) => void }).add(key, range, drawHighlight, {
          ...colors,
          tint: registry ? null : h.style.overlayTints[m.color],
          dot: m.note,
          range,
          multiply: h.style.multiply,
        })
        keys.push(key)
      }
      this.#hlKeys.set(layer, keys)
      if (registry) {
        const Highlight = (
          c.doc.defaultView as unknown as { Highlight: new (...r: Range[]) => unknown }
        ).Highlight
        for (const color of ['yellow', 'green', 'blue', 'rose'] as const) {
          const ranges = byColor[color]
          if (ranges?.length) registry.set(`linen-hl-${color}`, new Highlight(...ranges))
          else registry.delete(`linen-hl-${color}`)
        }
        let style = c.doc.getElementById('linen-highlight-style')
        if (!style) {
          style = c.doc.createElement('style')
          style.id = 'linen-highlight-style'
          c.doc.head?.append(style)
        }
        style.textContent = h
          ? Object.entries(h.style.colors)
              .map(
                ([color, v]) => `::highlight(linen-hl-${color}) { background-color: ${v.tint}; }`,
              )
              .join('\n')
          : ''
      }
    }
  }

  /** V8, A8: pulse a passage for 1.2 s after a jump to it. */
  pulse(cfi: string, color: string) {
    const view = this.#current
    for (const c of view.renderer?.getContents() ?? []) {
      const layer = c.overlayer as unknown as
        { add: (...a: unknown[]) => void; remove: (k: string) => void } | undefined
      if (!layer || !c.doc) continue
      const range = this.#cfiRange(view, cfi, c.doc, c.index)
      if (!range) continue
      layer.add('linen-pulse', range, drawPulse, { color, range })
      setTimeout(() => layer.remove('linen-pulse'), 1300)
    }
  }

  /** A1: report the selection in `doc` (or its end). */
  #reportSelection(view: View, doc: Document, index: number) {
    if (view !== this.#current) return
    const sel = doc.getSelection()
    const range = sel && sel.rangeCount && !sel.isCollapsed ? sel.getRangeAt(0) : null
    const text = range?.toString() ?? ''
    if (!range || !text.trim()) {
      if (this.#hadSelection) this.#selection.forEach((l) => l(null))
      this.#hadSelection = false
      return
    }
    const rects = this.#screenRects(range)
    let x = this.#texts.get(doc)
    if (!x) {
      x = extractText(doc.body ?? doc.documentElement)
      this.#texts.set(doc, x)
    }
    const start = offsetAt(x, range.startContainer, range.startOffset)
    const end = offsetAt(x, range.endContainer, range.endOffset)
    if (!rects || start === null || end === null || end <= start) return
    this.#hadSelection = true
    const event = {
      index,
      range,
      cfi: view.getCFI(index, range),
      start,
      end,
      text,
      quote: quoteAt(x.text, start, end),
      ...rects,
    }
    this.#selection.forEach((l) => l(event))
  }

  /**
   * L18: the share of the book's text that is code or tables, measured over every
   * chapter, one at a time in idle time (it runs once per book; the reader keeps it).
   */
  async codeShare(): Promise<number> {
    let code = 0
    let all = 0
    for (const section of this.#book?.sections ?? []) {
      if (section.linear === 'no') continue
      // WebKit has no requestIdleCallback: space the chapters out so this never
      // competes with reading or search (it runs once per book).
      await new Promise((r) => setTimeout(r, 50))
      // The book was closed: stop (the reader measures it again next time).
      if (this.#closed) return Number.NaN
      const doc = await section.createDocument?.().catch(() => null)
      const body = doc?.body
      if (!body) continue
      all += body.textContent?.length ?? 0
      // Outermost code blocks and tables only, so nested ones are not counted twice.
      for (const el of body.querySelectorAll('pre, table, code'))
        if (!el.parentElement?.closest('pre, table, code')) code += el.textContent?.length ?? 0
    }
    return all ? code / all : 0
  }

  /** Phase 7 (book.text): the reading text of one chapter, as search extracts it. */
  async chapterText(index: number): Promise<string> {
    const section = this.#book?.sections[index]
    if (!section) throw new Error(`there is no chapter ${index}`)
    const doc = await section.createDocument()
    const root = doc.body ?? doc.documentElement
    return root ? extractText(root).text : ''
  }

  /** Phase 7 (book.text): the book's chapters in reading order. */
  get chapterCount(): number {
    return this.#book?.sections.length ?? 0
  }

  /** A8: the chapter a CFI points into, or null if it does not resolve in this book. */
  cfiIndex(cfi: string): number | null {
    try {
      const r = this.#current.resolveNavigation(cfi) as { index: number } | undefined
      return r && Number.isInteger(r.index) && this.#book?.sections[r.index] ? r.index : null
    } catch {
      return null
    }
  }

  /**
   * A9, B3: place text quotes in this edition of the book. Each is tried in its old
   * CFI's chapter first, with the old position as a tie-break; then in every other
   * chapter, where only a single placement in the whole book counts. Null means
   * “Couldn't place”; nothing is put at a guessed position.
   */
  async placeQuotes(
    items: { quote: TextQuote; cfi: string }[],
  ): Promise<({ index: number; cfi: string; quote: TextQuote; how: Placement['how'] } | null)[]> {
    const view = this.#current
    const sections = this.#book?.sections ?? []
    const chapters = new Map<number, Promise<{ doc: Document; x: ExtractedText } | null>>()
    const chapter = (index: number) => {
      let c = chapters.get(index)
      if (!c) {
        c = (async () => {
          const doc = await sections[index]?.createDocument?.().catch(() => null)
          const root = doc?.body ?? doc?.documentElement
          return doc && root ? { doc, x: extractText(root) } : null
        })()
        chapters.set(index, c)
      }
      return c
    }
    const place = async (index: number, quote: TextQuote, oldCfi: string | null) => {
      const c = await chapter(index)
      if (!c) return null
      let hint: number | undefined
      if (oldCfi)
        try {
          const r = this.#cfiRange(view, oldCfi, c.doc, index)
          hint = r ? (offsetAt(c.x, r.startContainer, r.startOffset) ?? undefined) : undefined
        } catch {
          hint = undefined
        }
      const p = locate(c.x.text, quote, hint)
      const range = p && rangeFor(c.x, p.start, p.end)
      return p && range
        ? {
            index,
            cfi: view.getCFI(index, range),
            quote: quoteAt(c.x.text, p.start, p.end),
            how: p.how,
          }
        : null
    }
    const out = []
    for (const { quote, cfi } of items) {
      const first = this.cfiIndex(cfi)
      let found = first === null ? null : await place(first, quote, cfi)
      if (!found) {
        const elsewhere = []
        for (let i = 0; i < sections.length && elsewhere.length < 2; i++)
          if (i !== first) {
            const p = await place(i, quote, null)
            if (p) elsewhere.push(p)
          }
        // The same passage in two chapters: do not guess.
        found = elsewhere.length === 1 ? elsewhere[0] : null
      }
      out.push(found)
    }
    return out
  }

  /**
   * A6: where a highlight is on screen, in window coordinates: its first and last
   * lines and the edge of the text column beside the first (for the margin note).
   */
  passageBox(cfi: string): { first: DOMRect; last: DOMRect; edge: number } | null {
    const view = this.#current
    for (const c of view.renderer?.getContents() ?? []) {
      if (!c.doc) continue
      const range = this.#cfiRange(view, cfi, c.doc, c.index)
      if (!range) continue
      const rects = textRects(range)
      const frame = c.doc.defaultView?.frameElement?.getBoundingClientRect()
      if (!rects.length || !frame) return null
      const rtl = c.doc.body
        ? c.doc.defaultView?.getComputedStyle(c.doc.body).direction === 'rtl'
        : false
      const move = (r: DOMRect) =>
        new DOMRect(r.left + frame.left, r.top + frame.top, r.width, r.height)
      const firstVisible =
        rects.find((r) => r.right + frame.left > 0 && r.left + frame.left < window.innerWidth) ??
        rects[0]
      return {
        first: move(firstVisible),
        last: move(rects[rects.length - 1]),
        edge: columnEdge(range, firstVisible, !!rtl) + frame.left,
      }
    }
    return null
  }

  /** F5: a Range for extracted-text offsets in a live document (same extractor as the index). */
  textRange(doc: Document, start: number, end: number): Range | null {
    let text = this.#texts.get(doc)
    if (!text) {
      text = extractText(doc.body ?? doc.documentElement)
      this.#texts.set(doc, text)
    }
    return rangeFor(text, start, end)
  }

  /** The selected text in the book, to pre-fill search (F1). */
  get selectionText(): string {
    return this.#docOf(this.#current)?.getSelection()?.toString().replace(/\s+/g, ' ').trim() ?? ''
  }

  /**
   * F4–F6: go to a search match, given as offsets into the chapter's extracted
   * text. Lays out the chunk holding it (L16) and shows it if it is in a footnote
   * taken out of the flow (N9). Returns false when the match cannot be placed.
   */
  async goToText(index: number, start: number, end: number): Promise<boolean> {
    // B8: in Scroll mode this navigation supersedes any stack still being built.
    const seq = this.#mode === 'scroll' ? ++this.#stackSeq : 0
    const view = seq ? this.#scrollViewFor(index) : this.#current
    const node = (doc: Document) => this.textRange(doc, start, end)?.startContainer ?? null
    if (this.#indexOf(view) !== index) {
      this.#requests.set(view, { kind: 'node', node })
      await view.goTo(index)
    } else {
      const c = this.#chunksOf(view)
      const doc = this.#docOf(view)
      if (c && doc) {
        const k = chunkOf(c, node(doc))
        if (k !== c.current) {
          showChunk(doc, c, k)
          view.renderer.render()
          await nextFrame()
        }
      }
    }
    const doc = this.#docOf(view)
    const range = doc ? this.textRange(doc, start, end) : null
    if (!range) {
      // The stack may be hidden for the load: show the section rather than nothing.
      if (seq === this.#stackSeq && seq) await this.#scrollToTarget(view, index, seq)
      return false
    }
    const el =
      range.startContainer.nodeType === 1
        ? (range.startContainer as Element)
        : range.startContainer.parentElement
    el?.closest('[data-linen-footnote]')?.removeAttribute('data-linen-footnote')
    if (seq) {
      if (seq === this.#stackSeq) await this.#scrollToTarget(view, range, seq)
    } else {
      await view.renderer.goTo({ index, anchor: range })
      this.#prepareNeighbours()
    }
    this.#drawMarks(view)
    return true
  }

  /** F5: show search marks (null removes them; marks exist only while Search is open). */
  setSearchMarks(marks: SearchMarks | null) {
    this.#marks = marks
    for (const view of this.#views()) this.#drawMarks(view)
  }

  #drawMarks(view: View) {
    const t0 = performance.now()
    if (view === this.#current) this.#markStats = { drawn: 0, ms: 0, active: null, soft: null }
    for (const c of view.renderer?.getContents() ?? []) {
      const layer = c.overlayer as MarkLayer | undefined
      if (!layer || !c.doc) continue
      const m = this.#marks
      const list = m?.bySection.get(c.index) ?? []
      // Results stream in chapter by chapter: most updates change nothing on this page.
      const activeKey = m?.active?.index === c.index ? `${m.active.start}:${m.active.end}` : ''
      const last = this.#markDrawn.get(layer)
      if (last && last.list === list && last.active === activeKey && !!last.set === !!m) continue
      this.#markDrawn.set(layer, { list, active: activeKey, set: m })
      for (const key of this.#markKeys.get(layer) ?? []) layer.remove(key)
      const keys: string[] = []
      let activeRange: Range | null = null
      list.forEach((mark, i) => {
        const range = this.textRange(c.doc, mark.start, mark.end)
        if (!range || !m) return
        const active =
          m.active?.index === c.index && m.active.start === mark.start && m.active.end === mark.end
        if (active) return void (activeRange = range)
        const key = `linen-search:${i}`
        const style = { fill: m.colors.tint, stroke: m.colors.outline, width: 1 }
        layer.add(key, range, drawMark, style)
        keys.push(key)
        if (view === this.#current) {
          this.#markStats.drawn++
          this.#markStats.soft = style
        }
      })
      // The active match last, so it sits on top.
      if (activeRange && m) {
        const style = { fill: m.colors.activeTint, stroke: m.colors.activeOutline, width: 2 }
        layer.add('linen-search:active', activeRange, drawMark, style)
        keys.push('linen-search:active')
        if (view === this.#current) {
          this.#markStats.drawn++
          this.#markStats.active = style
        }
      }
      this.#markKeys.set(layer, keys)
    }
    if (view === this.#current) this.#markStats.ms += Math.round(performance.now() - t0)
  }

  /** N9 “Open note in place”: go to the note, showing it if it was taken out of the flow. */
  async openNoteInPlace(note: NoteEvent) {
    note.note?.removeAttribute('data-linen-footnote')
    await this.#navigate(note.href)
  }

  onLink(l: Listener<LinkEvent>) {
    this.#link.add(l)
    return () => this.#link.delete(l)
  }

  get mode(): ReadingMode {
    return this.#sideways ? 'scroll' : this.#mode
  }

  /**
   * B8, G8: switch between Pages and Scroll, keeping the place. Fixed-layout and
   * vertical books stay in Pages. Returns whether the mode is now `mode`.
   */
  async setMode(mode: ReadingMode): Promise<boolean> {
    if (this.fixedLayout && mode === 'scroll') return false
    // I16: vertical writing scrolls sideways in its own flow.
    if (mode === 'scroll' && this.#vertical() && this.#mode === 'pages') {
      if (this.#sideways) return true
      return this.#setSideways(true)
    }
    if (mode === 'pages' && this.#sideways) return this.#setSideways(false)
    if (mode === this.#mode) return true
    const cfi = this.location?.cfi
    if (mode === 'scroll') this.#enterScroll()
    else this.#leaveScroll()
    const layout = this.#layout
    if (layout && mode === 'pages') this.applyLayout(layout, this.#styles)
    for (const view of this.#views()) if (view.renderer) this.#configureRenderer(view)
    await this.#navigate(cfi ?? this.#textStart())
    return true
  }

  /**
   * Scroll mode: scroll by a distance from AppKit's scroll stream (wheel and trackpad,
   * momentum included). WebKit does not pass the wheel from the book's frames to
   * the host scroller, so the engine scrolls it (found with real posted events).
   */
  scrollPixels(dy: number) {
    if (this.#sideways) return this.#scrollSideways(dy)
    if (this.#mode !== 'scroll') return
    this.#lastInputAt = performance.now()
    this.#scrollPin = null
    this.#host.scrollTop += dy
  }

  /** I9: scroll by lines in Scroll mode (↓ ↑). */
  scrollLines(n: number) {
    if (!this.#layout) return
    if (this.#sideways) return this.#scrollSideways(n * this.#layout.lineHeightPx)
    if (this.#mode !== 'scroll') return
    this.#lastInputAt = performance.now()
    this.#scrollPin = null
    this.#host.scrollBy(0, n * this.#layout.lineHeightPx)
  }

  async #setSideways(on: boolean): Promise<boolean> {
    const cfi = this.location?.cfi
    this.#sideways = on
    for (const view of this.#views()) if (view.renderer) this.#configureRenderer(view)
    await this.#navigate(cfi ?? this.#textStart())
    return true
  }

  /**
   * I16: sideways scrolling. foliate's `next(distance)` and `prev(distance)` scroll the
   * scrolled flow and, at either end of the chapter, load the next or previous one.
   * A call runs at a time; deltas that arrive meanwhile add up and follow.
   */
  #sidewaysPending = 0
  #sidewaysBusy = false
  #scrollSideways(delta: number) {
    this.#lastInputAt = performance.now()
    this.#sidewaysPending += delta
    if (this.#sidewaysBusy) return
    void this.#drainSideways()
  }
  async #drainSideways() {
    this.#sidewaysBusy = true
    try {
      while (Math.abs(this.#sidewaysPending) >= 1 && this.#sideways && !this.#closed) {
        const d = this.#sidewaysPending
        this.#sidewaysPending = 0
        const r = this.#current.renderer as unknown as {
          next(distance: number): Promise<void>
          prev(distance: number): Promise<void>
        }
        await (d > 0 ? r.next(d) : r.prev(-d))
      }
    } finally {
      this.#sidewaysPending = 0
      this.#sidewaysBusy = false
    }
  }

  /** I17: zoom of a fixed-layout page relative to fit (1 = fit). */
  get zoom(): number {
    return this.#zoom
  }

  /**
   * I17, G8: zoom fixed-layout pages, keeping the centre of the view in place.
   * foliate-js takes an absolute scale, so the fit scale is read from the page first.
   */
  setZoom(zoom: number) {
    const r = this.#current.renderer
    if (!this.fixedLayout || !r) return
    const next = Math.min(ZOOM_LEVELS[ZOOM_LEVELS.length - 1], Math.max(1, zoom))
    if (next === this.#zoom) return
    if (this.#zoom === 1) this.#fitScale = this.#pageScale() ?? this.#fitScale
    // Keep the point at the centre of the view where it is.
    const cx = (r.scrollLeft + r.clientWidth / 2) / Math.max(1, r.scrollWidth)
    const cy = (r.scrollTop + r.clientHeight / 2) / Math.max(1, r.scrollHeight)
    this.#zoom = next
    // G8: a zoomed page takes the whole window; at fit it returns inside the margins.
    Object.assign(
      this.#current.style,
      next === 1
        ? {
            left: `${MIN_SIDE_MARGIN}px`,
            top: `${FIXED_VERTICAL_MARGIN}px`,
            width: `calc(100% - ${2 * MIN_SIDE_MARGIN}px)`,
            height: `calc(100% - ${2 * FIXED_VERTICAL_MARGIN}px)`,
          }
        : { left: '0', top: '0', width: '100%', height: '100%' },
    )
    r.setAttribute('zoom', next === 1 ? 'fit-page' : String(this.#fitScale * next))
    r.scrollLeft = cx * r.scrollWidth - r.clientWidth / 2
    r.scrollTop = cy * r.scrollHeight - r.clientHeight / 2
  }

  /** Pan a zoomed page by a distance in screen pixels. */
  pan(dx: number, dy: number) {
    const r = this.#current.renderer
    if (this.#zoom > 1 && r) r.scrollBy(dx, dy)
  }

  /** The scale foliate-js applied to the current fixed-layout page. */
  #pageScale(): number | null {
    const frame = this.#current.renderer?.getContents()[0]?.doc?.defaultView?.frameElement as
      HTMLElement | null | undefined
    const m = frame && /scale\(([\d.]+)\)/.exec(frame.style.transform)
    return m ? Number(m[1]) : null
  }

  /** Focus the page (the book text), e.g. when a layer closes (S5). */
  focusPage() {
    // D1: called on a frame after a reveal; the paginator may have no view yet (or any more).
    if (this.#closed) return
    try {
      this.#current.renderer?.focusView?.()
    } catch {
      // Nothing shown to focus.
    }
  }

  /** Open a book from a file, or (L17) from a loader that reads entries on demand. */
  async open(
    source: File | EntryLoader,
    start?: { cfi?: string; mode?: ReadingMode },
  ): Promise<Book> {
    let book: Book
    if (source instanceof File) {
      const { makeBook } = await import('foliate-js/view.js')
      book = await makeBook(source)
    } else {
      const { EPUB } = await import('foliate-js/epub.js')
      book = await new EPUB(source).init()
    }
    book.transformTarget?.addEventListener('data', (e) => {
      const detail = (e as CustomEvent<{ data: unknown; type: string }>).detail
      const type = detail.type
      detail.data = Promise.resolve(detail.data)
        .then((data) => transformContent(data, type))
        .catch(() => '') // a damaged resource must not break the section (E3)
    })
    this.#book = book
    await this.#current.open(book)
    // One book, three views: the visible one and its two neighbours (D-D1).
    // Fixed-layout books use one view; their pages are images of whole spreads.
    if (!this.fixedLayout) {
      for (const view of [this.#next.view, this.#prev.view]) await view.open(book)
    }
    if (start?.mode === 'scroll' && !this.fixedLayout) this.#enterScroll()
    for (const view of this.#views()) if (view.renderer) this.#configureRenderer(view)
    if (this.#mode === 'scroll') {
      await this.#navigate(start?.cfi ?? this.#textStart())
      // I16: a vertical book remembered in Scroll scrolls sideways instead.
      if (this.#vertical()) {
        await this.setMode('pages')
        await this.setMode('scroll')
      }
      return book
    }
    if (start?.cfi) {
      // Resume: lay out the chunk that holds the saved place (L16).
      this.#request(this.#current, start.cfi)
      await this.#current.init({ lastLocation: start.cfi })
      await this.#settleChunkAnchor(this.#current, start.cfi)
    } else {
      // N3: new books open at the bodymatter landmark, not the cover.
      await this.#current.init({ showTextStart: true })
    }
    this.#prepareNeighbours()
    return book
  }

  /** Place the page box and tell the paginators their geometry (L1–L3, L8). */
  /**
   * X5: styles laid over the book's, as a user stylesheet would (WCAG 1.4.12 text
   * spacing is tested this way). The page re-paginates with them.
   */
  setExtraStyles(css: string) {
    this.#extraStyles = css
    if (this.#layout) this.applyLayout(this.#layout, this.#styles)
  }

  applyLayout(layout: Layout, styles: string) {
    this.#layout = layout
    this.#styles = styles
    if (this.#mode === 'scroll') {
      // Heights change with the width and font: lay the stack out again at the same
      // place. Until the reader moves, that is the place last asked for (B8).
      const pinned = this.#pinned()?.target
      const target = pinned !== undefined && !isRange(pinned) ? pinned : this.location?.cfi
      for (const view of this.#views()) if (view.renderer) this.#configureRenderer(view)
      if (this.#book && target !== undefined) void this.#navigate(target)
      return
    }
    for (const view of this.#views()) {
      // G8: fixed-layout pages fit the area inside 24 px side and 64 px top and bottom margins.
      const fixed = view.isFixedLayout === true
      Object.assign(view.style, {
        left: `${fixed ? MIN_SIDE_MARGIN : layout.viewLeft}px`,
        top: `${fixed ? FIXED_VERTICAL_MARGIN : layout.top}px`,
        width: fixed ? `calc(100% - ${2 * MIN_SIDE_MARGIN}px)` : `${layout.viewWidth}px`,
        height: fixed ? `calc(100% - ${2 * FIXED_VERTICAL_MARGIN}px)` : `${layout.pageHeight}px`,
      })
      // foliate-js creates its paginator in open(); until then the layout waits.
      if (view.renderer) this.#configureRenderer(view)
    }
    // A new window size means a new fit: zoom starts again from fit (I17).
    this.setZoom(1)
    // Reflow moves the neighbours' last and first pages: park them again.
    if (this.#book) this.#prepareNeighbours(true)
  }

  /**
   * C5: Publisher styles. Off turns the book's own stylesheets and style attributes
   * off in every loaded document (and in each one that loads later); the caller
   * lays the book out again.
   */
  setPublisherStyles(mode: PublisherStyles) {
    if (mode === this.#publisher) return
    this.#publisher = mode
    for (const view of this.#views())
      for (const c of view.renderer?.getContents?.() ?? []) if (c.doc) this.#applyPublisher(c.doc)
  }

  #applyPublisher(doc: Document) {
    const off = this.#publisher === 'off'
    for (const el of doc.querySelectorAll<HTMLStyleElement | HTMLLinkElement>(
      'style[data-linen-book], link[data-linen-book]',
    ))
      el.disabled = off
    // Inside the body only: foliate-js keeps its pagination styles on <html> and <body>.
    const body = doc.body
    if (!body) return
    if (off)
      for (const el of body.querySelectorAll('[style]')) {
        el.setAttribute('data-linen-style', el.getAttribute('style') ?? '')
        el.removeAttribute('style')
      }
    else
      for (const el of body.querySelectorAll('[data-linen-style]')) {
        el.setAttribute('style', el.getAttribute('data-linen-style') ?? '')
        el.removeAttribute('data-linen-style')
      }
  }

  setStyles(styles: string) {
    this.#styles = styles
    for (const view of this.#views())
      if (!view.isFixedLayout) view.renderer?.setStyles?.(styles + this.#extraStyles)
  }

  /** Turn a page. A turn requested while one is running is queued (at most one, I6). */
  turn(dir: Turn): Promise<void> {
    this.#lastInputAt = performance.now()
    this.#scrollPin = null
    if (this.#turning) {
      this.#pending = dir
      return this.#turning
    }
    this.#turning = this.#turnNow(dir).finally(() => {
      this.#turning = null
      const pending = this.#pending
      this.#pending = null
      if (pending) void this.turn(pending)
    })
    return this.#turning
  }

  /** Go to a CFI, an href or a section index, laying out the chunk that holds it (L16). */
  async goTo(target: string | number) {
    this.#note(`goTo ${target}`)
    await this.#navigate(target)
  }

  async goToFraction(fraction: number) {
    await this.#navigate({ fraction })
  }

  async goToTextStart() {
    if (this.#mode === 'scroll') return this.#navigate(this.#textStart())
    await this.#current.goToTextStart()
    this.#prepareNeighbours()
  }

  /** N3: the bodymatter landmark, or the first linear section (as foliate-js does). */
  #textStart(): string | number {
    const book = this.#book
    return (
      book?.landmarks?.find((m) => m.type.includes('bodymatter') || m.type.includes('text'))
        ?.href ??
      book?.sections.findIndex((s) => s.linear !== 'no') ??
      0
    )
  }

  async nextSection() {
    const index = this.#adjacent(this.#currentIndex(), 1)
    if (index >= 0) await this.#navigate(index)
  }

  async prevSection() {
    const index = this.#adjacent(this.#currentIndex(), -1)
    if (index >= 0) await this.#navigate(index)
  }

  /** N8: the section a book fraction lands in. */
  sectionAt(fraction: number): number {
    const r = this.#current.resolveNavigation({ fraction: Math.min(1, Math.max(0, fraction)) }) as
      { index: number } | undefined
    return r?.index ?? -1
  }

  /** N8: where a section starts, as a book fraction (by size, as foliate-js measures). */
  sectionStart(index: number): number {
    return this.#bookFraction(index, 0)
  }

  /** N8: the book's print page list (EPUB page-list), or an empty list. */
  get pageList(): { label: string; href: string }[] {
    const list = (this.#book as { pageList?: { label?: string; href?: string }[] } | null)?.pageList
    return (list ?? [])
      .filter((p) => p.label && p.href)
      .map((p) => ({ label: String(p.label).trim(), href: String(p.href) }))
  }

  get location(): ReaderLocation | null {
    const l = this.#current.lastLocation
    return l
      ? this.#toLocation(this.#current, l as unknown as Record<string, unknown>, 'navigation')
      : null
  }

  /**
   * B1: count each section's pages at the current layout in idle time, in a
   * hidden view, one section at a time; counting waits while pages are turning.
   * Chunked chapters (L16) are skipped: their pages stay estimates. Returns a
   * function that stops counting (call it before the layout changes).
   */
  countPages(want: (index: number) => boolean, report: (index: number, pages: number) => void) {
    const book = this.#book
    // Scroll mode has no pages to number.
    if (!book || this.fixedLayout || this.#mode === 'scroll') return () => {}
    let stopped = false
    const idle = async () => {
      // Safari has no requestIdleCallback: wait for a quiet moment instead.
      for (;;) {
        await new Promise((r) => setTimeout(r, COUNT_STEP_MS))
        if (stopped) return
        if (!this.#turning && performance.now() - this.#lastInputAt > COUNT_QUIET_MS) return
      }
    }
    void (async () => {
      if (!this.#counter) {
        const view = this.#createView()
        this.#counter = view
        await view.open(book)
        this.#configureRenderer(view)
        const layout = this.#layout
        if (layout)
          Object.assign(view.style, {
            left: `${layout.viewLeft}px`,
            top: `${layout.top}px`,
            width: `${layout.viewWidth}px`,
            height: `${layout.pageHeight}px`,
          })
      }
      const view = this.#counter
      for (let index = 0; index < book.sections.length && !stopped; index++) {
        const section = book.sections[index]
        if (section.linear === 'no' || section.size > CHUNK_THRESHOLD_BYTES || !want(index))
          continue
        await idle()
        if (stopped) return
        try {
          await view.renderer.goTo({ index, anchor: 0 })
          await nextFrame()
        } catch {
          continue // a damaged section keeps its estimate (E3)
        }
        const r = view.renderer
        if (!stopped && this.#indexOf(view) === index && r.pages > 2) report(index, r.pages - 2)
      }
    })()
    return () => {
      stopped = true
    }
  }

  /**
   * X3 (Spike C): VoiceOver scrolls the paginator to follow its cursor, and
   * foliate-js neither notices nor snaps to a page. While a screen reader runs,
   * watch the page and report the move.
   */
  watchExternalScroll(on: boolean) {
    cancelAnimationFrame(this.#watchTimer)
    if (!on) return
    const tick = () => {
      // In Scroll mode VoiceOver scrolls the host, and the scroll handler follows it.
      if (this.#mode === 'scroll') {
        this.#watchTimer = requestAnimationFrame(tick)
        return
      }
      const view = this.#current
      const r = view.renderer
      const page = r?.page ?? -1
      if (this.#lastPage !== -1 && page !== this.#lastPage && !this.#turning) {
        void r.goTo({
          index: r.getContents()[0]?.index,
          anchor: (page - 1) / Math.max(1, r.pages - 2),
        })
        const l = view.lastLocation
        if (l)
          this.#relocate.forEach((f) =>
            f(this.#toLocation(view, l as unknown as Record<string, unknown>, 'external')),
          )
      }
      this.#lastPage = page
      this.#watchTimer = requestAnimationFrame(tick)
    }
    this.#watchTimer = requestAnimationFrame(tick)
  }

  close() {
    this.#closed = true
    cancelAnimationFrame(this.#watchTimer)
    cancelAnimationFrame(this.#scrollFrame)
    clearInterval(this.#heightTimer)
    for (const view of this.#views()) {
      // foliate-js throws when closing a view that never showed a section
      // (a neighbour of a very short book); remove it regardless.
      try {
        if (this.#indexOf(view) >= 0) view.close()
      } catch (e) {
        console.warn('closing a reader view failed', e)
      }
      view.remove()
    }
    this.#book?.destroy?.()
    this.#host.replaceChildren()
  }

  /** N9: find the note a reference points at, in this chapter or another. */
  async #peek(view: View, marker: Element, href: string) {
    const resolved = view.resolveNavigation(href) as
      { index: number; anchor?: (doc: Document) => Range | Element | null } | undefined
    let note: Element | null = null
    try {
      if (resolved?.anchor) {
        const doc =
          resolved.index === this.#indexOf(view)
            ? marker.ownerDocument
            : await this.#book?.sections[resolved.index]?.createDocument?.()
        const target = doc ? resolved.anchor(doc) : null
        const el =
          target && 'startContainer' in target
            ? target.startContainer.nodeType === 1
              ? (target.startContainer as Element)
              : target.startContainer.parentElement
            : (target as Element | null)
        note = el ? noteContainer(el) : null
      }
    } catch {
      note = null
    }
    const r = marker.getBoundingClientRect()
    const frame = marker.ownerDocument.defaultView?.frameElement?.getBoundingClientRect()
    const rect = new DOMRect(
      (frame?.left ?? 0) + r.left,
      (frame?.top ?? 0) + r.top,
      r.width,
      r.height,
    )
    const event = { marker, href, label: marker.textContent?.trim() ?? '', note, rect }
    this.#notes.forEach((l) => l(event))
  }

  /** Diagnostics for the end-to-end tests. */
  debug() {
    return {
      trail: [...this.#trail],
      views: this.#views().map((v) => this.#unitOf(v)),
      chunks: this.#views().map((v) => {
        const doc = this.#docOf(v)
        const c = doc ? this.#chunks.get(doc) : undefined
        return c ? `${c.current + 1}/${chunkCount(c)}` : '-'
      }),
      next: this.#next.unit,
      prev: this.#prev.unit,
      mode: this.#mode,
      marks: { ...this.#markStats },
      slots: this.#slots.map((x) => ({
        unit: `${x.unit.index}:${x.unit.chunk}`,
        top: Math.round(x.top),
        height: Math.round(x.height),
        current: x.view === this.#current,
      })),
    }
  }

  // ---------------------------------------------------------------- views

  #note(e: string) {
    this.#trail.push(`${Math.round(performance.now())} ${e}`)
    if (this.#trail.length > 60) this.#trail.shift()
  }

  #views(): View[] {
    const views = [this.#current, this.#next.view, this.#prev.view]
    return this.#counter ? [...views, this.#counter] : views
  }

  #createView(): View {
    const view = document.createElement('foliate-view') as View
    Object.assign(view.style, {
      position: 'absolute',
      display: 'block',
      visibility: 'hidden',
      pointerEvents: 'none',
    })
    // S12: foliate-js hides the pointer over the page after inactivity.
    view.setAttribute('autohide-cursor', '')
    // Hidden views stay out of the accessibility tree (visibility: hidden) and the tab order.
    view.setAttribute('inert', '')
    this.#host.append(view)
    view.addEventListener('relocate', (e) => {
      if (view !== this.#current) return
      // Scroll mode: foliate sees each unit whole; only the engine's own report counts.
      if (this.#mode === 'scroll' && !this.#emitting) return
      this.#onRelocate(view, (e as CustomEvent).detail)
    })
    view.addEventListener('external-link', (e) => {
      e.preventDefault()
      const href = String((e as CustomEvent<{ href_: string }>).detail.href_)
      this.#link.forEach((l) => l({ href, external: true }))
    })
    view.addEventListener('link', (e) => {
      // The engine follows internal links itself, so the target's chunk is laid out
      // (L16), and the app hears about the jump first (N1, N10).
      e.preventDefault()
      const { href: raw, a } = (e as CustomEvent<{ href: string; a?: Element }>).detail
      const href = String(raw)
      if (/^\s*javascript:/i.test(href)) return
      // N9: a note reference opens a peek; it never navigates.
      if (a && isNoteRef(a)) {
        void this.#peek(view, a, href)
        return
      }
      this.#link.forEach((l) => l({ href, external: false }))
      void this.#navigate(href)
    })
    view.addEventListener('load', (e) => {
      const { doc, index } = (e as CustomEvent<{ doc: Document; index: number }>).detail
      this.#onLoad(view, doc, index)
    })
    // A new document's overlay is ready: draw any search marks for it (F5).
    view.addEventListener('create-overlay', () => {
      this.#drawMarks(view)
      this.#drawHighlights(view)
      this.#drawCaret()
    })
    // A5: a plain click on a highlight (not the end of a drag that made a selection).
    view.addEventListener('show-annotation', (e) => {
      const { value, range } = (e as CustomEvent<{ value: string; range: Range }>).detail
      if (!value.startsWith('linen-hl:') || view !== this.#current) return
      const sel = range.startContainer.ownerDocument?.getSelection()
      if (sel && !sel.isCollapsed) return
      const rects = this.#screenRects(range)
      if (rects) this.#highlightClick.forEach((l) => l({ id: value.slice(9), ...rects }))
    })
    return view
  }

  #show(view: View) {
    for (const v of this.#views()) {
      const visible = v === view
      v.style.visibility = visible ? 'visible' : 'hidden'
      v.style.pointerEvents = visible ? '' : 'none'
      v.toggleAttribute('inert', !visible)
    }
  }

  #configureRenderer(view: View) {
    const layout = this.#layout
    const r = view.renderer
    if (!layout || !r) return
    // Fixed layout (E2): foliate-js scales pages to the view; no paginator settings or book styles.
    if (view.isFixedLayout) return
    if (this.#sideways && view !== this.#counter) {
      // I16: foliate's scrolled flow runs vertical text sideways, in the page box.
      r.setAttribute('flow', 'scrolled')
      r.setAttribute('margin', '0px')
      r.setAttribute('gap', '0%')
      r.setAttribute('max-column-count', '1')
      r.setAttribute('max-inline-size', `${Math.ceil(layout.pageHeight)}px`)
      r.setStyles(this.#styles + this.#extraStyles)
      return
    }
    if (this.#mode === 'scroll' && view !== this.#counter) {
      // B8: one unit at full height; the column is the measure, with no gap of its own.
      r.setAttribute('flow', 'scrolled')
      r.setAttribute('margin', '0px')
      r.setAttribute('gap', '0%')
      r.setAttribute('max-column-count', '1')
      r.setAttribute('max-inline-size', `${layout.columnWidth}px`)
      r.setStyles(this.#styles + this.#extraStyles)
      return
    }
    r.setAttribute('flow', 'paginated')
    r.setAttribute('margin', '0px')
    // The gap and view width together give the column and gutter: see layout.ts.
    r.setAttribute('gap', `${layout.gap * 100}%`)
    r.setAttribute('max-block-size', `${Math.ceil(layout.pageHeight)}px`)
    r.setAttribute('max-column-count', String(layout.columns))
    // One column fills the view; two columns split it (see layout.ts).
    r.setAttribute(
      'max-inline-size',
      `${layout.columns === 1 ? 100000 : Math.ceil(layout.viewWidth / 2)}px`,
    )
    r.setStyles(this.#styles + this.#extraStyles)
  }

  #docOf(view: View): Document | undefined {
    return view.renderer?.getContents()[0]?.doc
  }

  #indexOf(view: View): number {
    const c = view.renderer?.getContents()[0]
    // foliate-js's fixed-layout renderer leaves `index` out of its contents.
    return c?.index ?? (c?.doc ? this.#docIndex.get(c.doc) : undefined) ?? -1
  }

  #currentIndex(): number {
    return this.#indexOf(this.#current)
  }

  #unitOf(view: View): Unit | null {
    const index = this.#indexOf(view)
    if (index < 0) return null
    const doc = this.#docOf(view)
    const c = doc ? this.#chunks.get(doc) : undefined
    return { index, chunk: c ? c.current : 0 }
  }

  #chunksOf(view: View): Chunks | undefined {
    const doc = this.#docOf(view)
    return doc ? this.#chunks.get(doc) : undefined
  }

  // ---------------------------------------------------------------- navigation

  /** The next linear section after `index` in `dir` (foliate skips linear="no"), or -1. */
  #adjacent(index: number, dir: 1 | -1): number {
    const sections = this.#book?.sections ?? []
    for (let i = index + dir; i >= 0 && i < sections.length; i += dir)
      if (sections[i].linear !== 'no') return i
    return -1
  }

  /** The unit after or before the visible one: the next chunk, or the neighbouring section. */
  #adjacentUnit(dir: 1 | -1, view: View = this.#current): Unit | null {
    const unit = this.#unitOf(view)
    if (!unit) return null
    const c = this.#chunksOf(view)
    if (c) {
      const k = unit.chunk + dir
      if (k >= 0 && k < chunkCount(c)) return { index: unit.index, chunk: k }
    }
    const index = this.#adjacent(unit.index, dir)
    return index < 0 ? null : { index, chunk: dir === 1 ? 0 : -1 }
  }

  /** Is `view` showing `unit` (chunk -1 = the section's last chunk)? */
  #shows(view: View, unit: Unit): boolean {
    const u = this.#unitOf(view)
    if (!u || u.index !== unit.index) return false
    if (unit.chunk !== -1) return u.chunk === unit.chunk
    const c = this.#chunksOf(view)
    return !c || u.chunk === chunkCount(c) - 1
  }

  /** Tell `view` which chunk to lay out for `target` when its section loads. */
  #request(view: View, target: string | number | { fraction: number }) {
    if (typeof target === 'number') {
      this.#requests.set(view, { kind: 'chunk', chunk: 0 })
      return
    }
    const resolved = view.resolveNavigation(target) as
      { index: number; anchor?: number | ((doc: Document) => Range | Element | null) } | undefined
    const anchor = resolved?.anchor
    if (typeof anchor === 'number') {
      this.#requests.set(view, { kind: 'fraction', fraction: anchor })
    } else if (typeof anchor === 'function') {
      this.#requests.set(view, {
        kind: 'node',
        node: (doc) => {
          const a = anchor(doc)
          // The Range comes from the book's frame, another realm: no `instanceof Range`.
          return a && 'startContainer' in a ? a.startContainer : a
        },
      })
    }
  }

  /** Show the chunk `request` asks for in a loaded document; returns the chunk. */
  #applyRequest(doc: Document, c: Chunks, request: ChunkRequest | undefined): number {
    let k = 0
    if (request?.kind === 'chunk') k = Math.min(request.chunk, chunkCount(c) - 1)
    else if (request?.kind === 'last') k = chunkCount(c) - 1
    else if (request?.kind === 'node') k = chunkOf(c, request.node(doc))
    else if (request?.kind === 'fraction') k = this.#chunkAtFraction(c, request.fraction)
    if (k !== c.current) showChunk(doc, c, k)
    return k
  }

  #chunkAtFraction(c: Chunks, fraction: number): number {
    for (let k = chunkCount(c) - 1; k >= 0; k--) if (sectionFraction(c, k, 0) <= fraction) return k
    return 0
  }

  /**
   * Navigate the visible view to a target. For a chunked section the chunk that
   * holds the target is laid out first, so foliate never anchors to hidden text.
   */
  async #navigate(target: string | number | { fraction: number }) {
    const seq = this.#mode === 'scroll' ? ++this.#stackSeq : 0
    if (seq) this.#scrollPin = { target, view: null, y: NaN }
    const resolved = this.#current.resolveNavigation(target) as { index: number } | undefined
    if (!resolved) return
    const view = seq ? this.#scrollViewFor(resolved.index) : this.#current
    if (this.#indexOf(view) === resolved.index) {
      const c = this.#chunksOf(view)
      if (c) {
        this.#request(view, target)
        const doc = this.#docOf(view)!
        const before = c.current
        this.#applyRequest(doc, c, this.#requests.get(view))
        this.#requests.delete(view)
        if (c.current !== before) view.renderer.render()
      }
    } else {
      this.#request(view, target)
    }
    if (typeof target === 'object') {
      await view.goToFraction(target.fraction)
    } else {
      await view.goTo(target)
    }
    if (this.#mode === 'scroll') {
      if (seq === this.#stackSeq) await this.#scrollToTarget(view, target, seq)
      return
    }
    await this.#settleChunkAnchor(view, target)
    this.#prepareNeighbours()
  }

  /** After a fraction jump into a chunked section, anchor within the chunk rather than the section. */
  async #settleChunkAnchor(view: View, target: string | number | { fraction: number }) {
    if (typeof target !== 'object') return
    const c = this.#chunksOf(view)
    if (!c) return
    const resolved = view.resolveNavigation(target) as { index: number; anchor?: number }
    const f = typeof resolved?.anchor === 'number' ? resolved.anchor : 0
    const start = sectionFraction(c, c.current, 0)
    const end = sectionFraction(c, c.current, 1)
    const inChunk = end > start ? (f - start) / (end - start) : 0
    await view.renderer.goTo({ index: resolved.index, anchor: Math.max(0, Math.min(1, inChunk)) })
  }

  /** Lay out `unit` in `view` at `anchor` (0 = first page, 1 = last page). */
  async #park(view: View, unit: Unit, anchor: number) {
    if (this.#indexOf(view) === unit.index) {
      const c = this.#chunksOf(view)
      if (c) {
        const k = unit.chunk === -1 ? chunkCount(c) - 1 : unit.chunk
        if (k !== c.current) {
          showChunk(this.#docOf(view)!, c, k)
          view.renderer.render()
          await nextFrame()
        }
      }
    } else {
      this.#requests.set(
        view,
        unit.chunk === -1 ? { kind: 'last' } : { kind: 'chunk', chunk: unit.chunk },
      )
    }
    await view.renderer.goTo({ index: unit.index, anchor })
  }

  /** Park the hidden views at the first page of the next unit and the last page of the previous one. */
  #prepareNeighbours(force = false) {
    if (this.fixedLayout || this.#mode === 'scroll' || this.#currentIndex() < 0) return
    const place = (n: Neighbour, unit: Unit | null, anchor: number) => {
      if (!unit) {
        n.unit = null
        return
      }
      if (!force && n.unit && this.#shows(n.view, unit) && n.unit.index === unit.index) return
      n.unit = null
      const view = n.view
      this.#note(`park ${n === this.#next ? 'next' : 'prev'} -> ${unit.index}:${unit.chunk}`)
      // foliate-js ignores goTo while a view is locked after a turn, and still
      // resolves: record what the view actually shows, not what was asked.
      n.ready = this.#park(view, unit, anchor)
        .then(() => {
          if (n.view === view && this.#shows(view, unit)) n.unit = unit
        })
        .catch(() => {
          if (n.view === view) n.unit = null
        })
    }
    place(this.#next, this.#adjacentUnit(1), 0)
    place(this.#prev, this.#adjacentUnit(-1), 1)
  }

  async #turnNow(dir: Turn) {
    if (this.#mode === 'scroll') {
      // I9: Space and PgDn move a screen with two lines of overlap (the fades hide the edges).
      const h = this.#host.clientHeight - SCROLL_FADE_TOP - SCROLL_FADE_BOTTOM
      const step = Math.max(40, h - 2 * (this.#layout?.lineHeightPx ?? 30))
      this.#host.scrollBy(0, dir === 'next' ? step : -step)
      await nextFrame()
      return
    }
    if (this.fixedLayout) {
      // G8: a page turn while zoomed returns to fit.
      this.setZoom(1)
      await (dir === 'next' ? this.#current.next() : this.#current.prev())
      return
    }
    const r = this.#current.renderer
    if (!r?.getContents().length) {
      this.#note('turn ignored: the visible view shows nothing')
      return
    }
    const crossing = dir === 'next' ? r.page >= r.pages - 2 : r.page <= 1
    if (!crossing) {
      await (dir === 'next' ? this.#current.next() : this.#current.prev())
      return
    }
    const target = this.#adjacentUnit(dir === 'next' ? 1 : -1)
    if (!target) return // the start or end of the book
    const neighbour = dir === 'next' ? this.#next : this.#prev
    if (neighbour.unit && this.#shows(neighbour.view, target)) {
      // D-D1: the neighbour is laid out at the right page; swapping is the turn.
      this.#note(`swap ${dir} to ${target.index}:${target.chunk}`)
      const old = this.#current
      this.#current = neighbour.view
      this.#show(this.#current)
      // The old view becomes the neighbour on the other side, already at the right page.
      const other = dir === 'next' ? this.#prev : this.#next
      const recycled = other.view
      other.view = old
      other.unit = this.#unitOf(old)
      neighbour.view = recycled
      neighbour.unit = null
      const l = this.#current.lastLocation
      if (l) this.#onRelocate(this.#current, l as unknown as Record<string, unknown>, 'page')
      // Load the new far neighbour after this frame, off the turn.
      requestAnimationFrame(() => this.#prepareNeighbours())
      return
    }
    // The neighbour is not ready: lay the unit out now (slower, but correct).
    this.#note(`turn ${dir} without a ready neighbour`)
    await this.#park(this.#current, target, dir === 'next' ? 0 : 1)
    this.#prepareNeighbours()
  }

  // ---------------------------------------------------------------- Scroll mode (B8, G8)

  #vertical(): boolean {
    const doc = this.#docOf(this.#current)
    const mode = doc?.defaultView?.getComputedStyle(doc.documentElement).writingMode ?? ''
    return mode.startsWith('vertical')
  }

  /** Scroll mode scrolls from the native stream only; the host's own wheel scrolling would double it. */
  #blockWheel = (e: WheelEvent) => e.preventDefault()

  #enterScroll() {
    this.#mode = 'scroll'
    this.#host.addEventListener('wheel', this.#blockWheel, { passive: false })
    this.#host.style.overflowY = 'auto'
    this.#host.style.overflowX = 'hidden'
    this.#spacer ??= Object.assign(document.createElement('div'), { className: 'linen-spacer' })
    Object.assign(this.#spacer.style, { position: 'absolute', left: '0', width: '1px', top: '0' })
    this.#host.append(this.#spacer)
    // Heights change as fonts and images arrive; keep the stack in step.
    this.#heightTimer = window.setInterval(() => this.#checkHeights(), 250)
  }

  #leaveScroll() {
    this.#mode = 'pages'
    this.#scrollPin = null
    this.#host.removeEventListener('wheel', this.#blockWheel)
    clearInterval(this.#heightTimer)
    this.#host.scrollTop = 0
    this.#host.style.overflowY = ''
    this.#host.style.overflowX = ''
    this.#spacer?.remove()
    for (const j of this.#joins) j.remove()
    this.#joins = []
    this.#slots = []
    this.#next.unit = null
    this.#prev.unit = null
    this.#show(this.#current)
  }

  /** Lay a unit out in `view` at full height and measure it. */
  async #loadUnit(view: View, unit: Unit): Promise<Slot> {
    await this.#park(view, unit, 0)
    await this.#settled(view)
    return { view, unit: this.#unitOf(view) ?? unit, top: 0, height: view.renderer.viewSize }
  }

  /** Wait until a view's fonts have loaded (at most 1 s) and its height has stopped changing. */
  async #settled(view: View) {
    const doc = this.#docOf(view)
    if (doc?.fonts) await Promise.race([doc.fonts.ready, new Promise((r) => setTimeout(r, 1000))])
    let h = -1
    for (let i = 0; i < 10 && h !== view.renderer.viewSize; i++) {
      h = view.renderer.viewSize
      await nextFrame()
    }
  }

  /**
   * Scroll mode: the view a navigation to section `index` uses. A section already in
   * the stack is scrolled to, not loaded again. Otherwise the current view loads it,
   * and the stack is hidden until the target is in place, so the new section never
   * shows where the old one was and then jumps (B8).
   */
  #scrollViewFor(index: number): View {
    const shown = this.#slots.find((x) => this.#indexOf(x.view) === index)
    if (shown) return shown.view
    for (const x of this.#slots) x.view.style.visibility = 'hidden'
    for (const j of this.#joins) j.remove()
    this.#joins = []
    return this.#current
  }

  /**
   * Scroll mode: make the view the reader is in current. The neighbours keep the other
   * two views, so the three views stay distinct (`#views`); a navigation that found
   * the same view twice there lost one and stopped halfway.
   */
  #makeCurrent(view: View) {
    const old = this.#current
    if (view === old) return
    if (view === this.#next.view) this.#next.view = old
    else if (view === this.#prev.view) this.#prev.view = old
    this.#next.unit = null
    this.#prev.unit = null
    this.#current = view
  }

  /**
   * Scroll mode after a navigation: bring the target to the top of the window. If the
   * stack already holds the target's unit and its neighbours, that is only a scroll.
   * Otherwise the target is placed alone first and its neighbours load out of sight,
   * then join it above and below without moving it.
   */
  async #scrollToTarget(view: View, target: ScrollTarget, seq = ++this.#stackSeq) {
    if (this.#scrollPin?.target !== target) this.#scrollPin = { target, view: null, y: NaN }
    const stale = () => seq !== this.#stackSeq
    this.#stackBusy = true
    try {
      await this.#settled(view)
      const unit = this.#unitOf(view)
      if (!unit || stale()) return
      this.#makeCurrent(view)
      if (this.#scrollPin) this.#scrollPin.view = view
      const before = this.#adjacentUnit(-1, view)
      const after = this.#adjacentUnit(1, view)
      const holds = (slot: Slot | undefined, u: Unit | null) =>
        u ? !!slot && slot.unit.index === u.index && slot.unit.chunk === u.chunk : !slot
      const k = this.#slots.findIndex((x) => x.view === view)
      const placed = this.#slots[k]
      if (
        placed &&
        holds(placed, unit) &&
        holds(this.#slots[k - 1], before) &&
        holds(this.#slots[k + 1], after)
      ) {
        // Heights change with a relayout: place the stack again around the target first.
        for (const x of this.#slots) x.height = x.view.renderer?.viewSize ?? x.height
        this.#placeSlots(k)
        this.#alignTo(placed, target)
        return
      }
      const centre: Slot = { view, unit, top: 0, height: view.renderer.viewSize }
      this.#slots = [centre]
      this.#placeSlots(0)
      this.#alignTo(centre, target)
      this.#emitScrollLocation()
      const others = this.#views().filter((v) => v !== view && v !== this.#counter)
      const slots: Slot[] = [centre]
      if (before) {
        const slot = await this.#loadUnit(others[0], before)
        if (stale()) return
        slots.unshift(slot)
      }
      if (after) {
        const slot = await this.#loadUnit(others[1], after)
        if (stale()) return
        slots.push(slot)
      }
      // Heights read now, after the neighbours loaded (fonts may have changed them).
      for (const x of slots) x.height = x.view.renderer?.viewSize ?? x.height
      this.#slots = slots
      // The target keeps its place on screen; the stack grows around it.
      this.#placeSlots(slots.indexOf(centre))
      // Until the reader moves, the target stays at the top (B8), e.g. if its height changed.
      if (this.#pinned()) this.#alignTo(centre, target)
    } finally {
      if (!stale()) this.#stackBusy = false
    }
    if (!stale()) this.#emitScrollLocation()
  }

  /** Scroll so that `target` (in `slot`) sits at the top of the window, below the fade. */
  #alignTo(slot: Slot, target: ScrollTarget) {
    const y = this.#targetOffset(slot.view, target)
    const layoutTop = this.#layout?.top ?? 88
    this.#host.scrollTop = y < 1 ? slot.top - layoutTop : slot.top + y - SCROLL_FADE_TOP
    if (this.#scrollPin) this.#scrollPin.y = this.#host.scrollTop
  }

  /** The pinned place (B8), while the reader has not moved away from it. */
  #pinned() {
    const p = this.#scrollPin
    if (p && !Number.isNaN(p.y) && Math.abs(this.#host.scrollTop - p.y) > 1) this.#scrollPin = null
    return this.#scrollPin
  }

  /** Where a navigation target sits within its view, in px from the unit's top. */
  #targetOffset(view: View, target: ScrollTarget): number {
    if (typeof target === 'number') return 0
    if (typeof target === 'object' && 'startContainer' in target)
      return Math.max(0, (target.getClientRects()[0] ?? target.getBoundingClientRect()).top)
    const doc = this.#docOf(view)
    const height = view.renderer.viewSize
    const resolved = view.resolveNavigation(target) as
      { anchor?: number | ((doc: Document) => Range | Element | null) } | undefined
    const anchor = resolved?.anchor
    if (typeof anchor === 'number') {
      const c = this.#chunksOf(view)
      if (!c) return anchor * height
      const [start, end] = [sectionFraction(c, c.current, 0), sectionFraction(c, c.current, 1)]
      return end > start ? Math.max(0, Math.min(1, (anchor - start) / (end - start))) * height : 0
    }
    if (typeof anchor !== 'function' || !doc) return 0
    const a = anchor(doc)
    if (!a) return 0
    const rect = ('getClientRects' in a ? a.getClientRects()[0] : null) ?? a.getBoundingClientRect()
    return Math.max(0, rect.top)
  }

  /**
   * Position the stacked views. Slot `anchor` keeps its top; the others follow it.
   * If there is no room above the first slot, everything moves down and the
   * scroll position with it, so the text on screen stays still.
   */
  #placeSlots(anchor: number) {
    const slots = this.#slots
    const layout = this.#layout
    if (!slots.length || !layout) return
    const gapBefore = (i: number) =>
      i > 0 && slots[i - 1].unit.index === slots[i].unit.index ? 0 : SCROLL_JOIN
    const top0 = slots[anchor].top || layout.top
    slots[anchor].top = top0
    for (let i = anchor + 1; i < slots.length; i++)
      slots[i].top = slots[i - 1].top + slots[i - 1].height + gapBefore(i)
    for (let i = anchor - 1; i >= 0; i--)
      slots[i].top = slots[i + 1].top - gapBefore(i + 1) - slots[i].height
    // The first unit of the book starts where a page would; otherwise keep room to grow upward.
    const first = slots[0]
    const atStart = !this.#adjacentUnit(-1, first.view)
    const want = atStart ? layout.top : Math.max(layout.top, first.top)
    const shift = want - first.top
    if (shift !== 0) {
      for (const x of slots) x.top += shift
      const pinned = this.#pinned()
      if (!atStart || shift > 0) this.#host.scrollTop += shift
      else this.#host.scrollTop = Math.max(0, this.#host.scrollTop + shift)
      if (pinned && !Number.isNaN(pinned.y)) pinned.y = this.#host.scrollTop
    }
    const used = new Set(slots.map((x) => x.view))
    for (const x of slots)
      Object.assign(x.view.style, {
        left: `${layout.left}px`,
        width: `${layout.columnWidth}px`,
        top: `${x.top}px`,
        height: `${x.height}px`,
        visibility: 'visible',
        pointerEvents: '',
      })
    for (const x of slots) x.view.toggleAttribute('inert', false)
    for (const v of this.#views())
      if (!used.has(v)) {
        v.style.visibility = 'hidden'
        v.style.pointerEvents = 'none'
        v.toggleAttribute('inert', true)
      }
    // G8: between chapters, 124 px of space and a 48 px hairline.
    for (const j of this.#joins) j.remove()
    this.#joins = []
    for (let i = 1; i < slots.length; i++) {
      if (!gapBefore(i)) continue
      const j = document.createElement('div')
      j.className = 'linen-join'
      Object.assign(j.style, {
        position: 'absolute',
        left: `${layout.left + layout.columnWidth / 2 - 24}px`,
        width: '48px',
        top: `${slots[i].top - SCROLL_JOIN / 2}px`,
        borderTop: '1px solid var(--hairline)',
      })
      this.#host.append(j)
      this.#joins.push(j)
    }
    const last = slots[slots.length - 1]
    this.#spacer!.style.height = `${last.top + last.height + this.#host.clientHeight / 2}px`
  }

  /** Keep the stack in step with content heights (fonts and images load late). */
  #checkHeights() {
    if (this.#mode !== 'scroll' || this.#stackBusy) return
    let changed = false
    for (const x of this.#slots) {
      const h = x.view.renderer?.viewSize ?? x.height
      if (Math.abs(h - x.height) > 0.5) {
        x.height = h
        changed = true
      }
    }
    if (!changed) return
    // Keep the slot under the reader still.
    const pinned = this.#pinned()
    const i = Math.max(0, this.#slotIndexAt(this.#host.scrollTop + SCROLL_FADE_TOP))
    this.#placeSlots(i)
    // B8: until the reader moves, the place asked for stays at the top as text above it grows.
    const slot = pinned?.view && this.#slots.find((x) => x.view === pinned.view)
    if (slot) this.#alignTo(slot, pinned.target)
  }

  #slotIndexAt(y: number): number {
    const slots = this.#slots
    for (let i = 0; i < slots.length; i++)
      if (y < slots[i].top + slots[i].height + (i + 1 < slots.length ? SCROLL_JOIN / 2 : Infinity))
        return i
    return slots.length - 1
  }

  #onScroll() {
    if (this.#mode !== 'scroll' || !this.#slots.length) return
    this.#pinned() // the reader moved: the pin goes
    this.#checkHeights()
    // The chapter under the middle of the window is the current one (G8 note 3).
    const i = this.#slotIndexAt(this.#host.scrollTop + this.#host.clientHeight / 2)
    this.#makeCurrent(this.#slots[i].view)
    this.#emitScrollLocation()
    if (!this.#stackBusy) void this.#slide(i)
  }

  /** Load the next unit below or the previous one above when the reader nears the stack's end. */
  async #slide(i: number) {
    const slots = this.#slots
    const forward = i === slots.length - 1
    const backward = i === 0
    if (!forward && !backward) return
    const edge = slots[i]
    const unit = this.#adjacentUnit(forward ? 1 : -1, edge.view)
    if (!unit) return
    this.#stackBusy = true
    try {
      const inUse = new Set(slots.map((x) => x.view))
      const free = this.#views().find((v) => v !== this.#counter && !inUse.has(v))
      // Three views at most: recycle the one at the far end.
      const view = free ?? (forward ? slots[0].view : slots[slots.length - 1].view)
      if (!free) {
        if (forward) slots.shift()
        else slots.pop()
      }
      const slot = await this.#loadUnit(view, unit)
      if (forward) slots.push(slot)
      else slots.unshift(slot)
      // The slot being read keeps its place on screen.
      this.#placeSlots(slots.indexOf(edge))
    } finally {
      this.#stackBusy = false
    }
  }

  /** Report the place at the top of the window: the first text below the top fade. */
  #emitScrollLocation() {
    const slots = this.#slots
    const slot = slots.find((x) => x.view === this.#current)
    const doc = slot && this.#docOf(slot.view)
    if (!slot || !doc) return
    const y = Math.max(this.#host.scrollTop + SCROLL_FADE_TOP, slot.top) - slot.top
    const x = (doc.documentElement.clientWidth || this.#layout?.columnWidth || 640) / 2
    let range: Range | null = null
    for (let dy = 0; dy < 240 && !range; dy += 12) range = doc.caretRangeFromPoint(x, y + dy)
    if (!range) {
      range = doc.createRange()
      range.selectNodeContents(doc.body ?? doc.documentElement)
      range.collapse(true)
    }
    this.#scrollInUnit = slot.height ? Math.min(1, y / slot.height) : 0
    this.#emitting = true
    try {
      slot.view.renderer.dispatchEvent(
        new CustomEvent('relocate', {
          detail: {
            reason: 'scroll',
            range,
            index: slot.unit.index,
            fraction: this.#scrollInUnit,
            size: slot.height ? this.#host.clientHeight / slot.height : 1,
          },
        }),
      )
    } finally {
      this.#emitting = false
    }
  }

  // ---------------------------------------------------------------- documents and locations

  #onLoad(view: View, doc: Document, index: number) {
    this.#docIndex.set(doc, index)
    // C5: before anything measures the document.
    if (this.#publisher === 'off' && !view.isFixedLayout) this.#applyPublisher(doc)
    // L16: a very long chapter lays out only the chunk it was asked for.
    const size = this.#book?.sections[index]?.size ?? 0
    const request = this.#requests.get(view)
    this.#requests.delete(view)
    if (size > CHUNK_THRESHOLD_BYTES && !view.isFixedLayout) {
      const c = computeChunks(doc)
      if (chunkCount(c) > 1) {
        this.#chunks.set(doc, c)
        this.#applyRequest(doc, c, request)
        this.#note(`chunked section ${index}: showing ${c.current + 1}/${chunkCount(c)}`)
      }
    }
    doc.addEventListener('keydown', (e) => this.#key.forEach((l) => l(e)))
    // A1: a selection is reported when the mouse or keyboard finishes making it.
    const report = () => setTimeout(() => this.#reportSelection(view, doc, index), 0)
    doc.addEventListener('pointerup', report)
    doc.addEventListener('keyup', (e) => {
      if (e.shiftKey || e.key === 'Shift') report()
    })
    doc.addEventListener('selectionchange', () => {
      if (this.#caret) this.#drawCaret()
      if (doc.getSelection()?.isCollapsed && this.#hadSelection) report()
    })
    // N9: footnote asides that the text refers to are read in the peek, not in the flow.
    const asides = referencedFootnoteAsides(doc)
    if (asides.length) {
      const style = doc.createElement('style')
      style.textContent = '[data-linen-footnote] { display: none !important; }'
      doc.head?.append(style)
      for (const el of asides) el.setAttribute('data-linen-footnote', '')
    }
    if (!view.isFixedLayout) {
      // N11: an image (not one inside a link) opens the image view.
      doc.addEventListener('click', (e) => {
        const el = (e.target as Element | null)?.closest?.('img, image')
        if (!el || el.closest('a[href]')) return
        const src =
          (el as HTMLImageElement).currentSrc ||
          el.getAttribute('src') ||
          el.getAttribute('href') ||
          el.getAttributeNS('http://www.w3.org/1999/xlink', 'href') ||
          ''
        if (!src) return
        const caption = el.closest('figure')?.querySelector('figcaption')?.textContent ?? ''
        const alt = el.getAttribute('alt') ?? ''
        const event = {
          src,
          alt,
          caption: (caption || alt).replace(/\s+/g, ' ').trim(),
          element: el,
        }
        this.#images.forEach((l) => l(event))
      })
      // N10: show where an external link goes.
      const external = (t: EventTarget | null) => {
        const a = (t as Element | null)?.closest?.('a[href]')
        const href = a?.getAttribute('href') ?? ''
        return /^(https?:|mailto:)/i.test(href) ? href : null
      }
      doc.addEventListener('mouseover', (e) => {
        const href = external(e.target)
        if (href) this.#hover.forEach((l) => l(href))
      })
      doc.addEventListener('mouseout', (e) => {
        if (external(e.target)) this.#hover.forEach((l) => l(null))
      })
    }
    // L12: wide tables and code scroll inside their own box.
    for (const el of doc.querySelectorAll('table, pre')) {
      if (el.parentElement?.classList.contains('linen-scroll')) continue
      const wrap = doc.createElement('div')
      wrap.className = 'linen-scroll'
      el.replaceWith(wrap)
      wrap.append(el)
    }
    // L6: keep the book's paragraph convention; add spacing when it has neither indent nor spacing.
    const ps = Array.from(doc.querySelectorAll('p')).slice(1, 6)
    const win = doc.defaultView
    if (win && ps.length >= 2) {
      const flat = ps.every((p) => {
        const cs = win.getComputedStyle(p)
        return (
          parseFloat(cs.textIndent) === 0 &&
          parseFloat(cs.marginTop) === 0 &&
          parseFloat(cs.marginBottom) === 0
        )
      })
      if (flat) {
        const style = doc.createElement('style')
        style.textContent = PARAGRAPH_SPACING_CSS
        doc.head?.append(style)
      }
    }
    if (this.#styles && !view.isFixedLayout)
      view.renderer?.setStyles?.(this.#styles + this.#extraStyles)
    this.#doc.forEach((l) => l(doc))
  }

  #onRelocate(view: View, detail: Record<string, unknown>, reason?: ReaderLocation['reason']) {
    this.#lastPage = view.renderer?.page ?? -1
    const loc = this.#toLocation(
      view,
      detail,
      reason ?? (detail.reason as ReaderLocation['reason']) ?? 'page',
    )
    this.#relocate.forEach((l) => l(loc))
    // A11: a page turned by other means takes the caret along (a selection stays put).
    if (this.#caret && view === this.#current) {
      const sel = this.#pageContents()?.doc.getSelection()
      const at = sel?.rangeCount ? sel.getRangeAt(0) : null
      if (!at || (at.collapsed && !this.#onPage(at))) this.#caretToPageStart()
      this.#drawCaret()
    }
  }

  #toLocation(
    view: View,
    d: Record<string, unknown>,
    reason: ReaderLocation['reason'],
  ): ReaderLocation {
    const section = (d.section as { current: number; total: number } | undefined) ?? {
      current: 0,
      total: 1,
    }
    const time = d.time as { section?: number } | undefined
    const r = view.renderer
    const pages = r && !r.scrolled && r.pages > 2 ? r.pages - 2 : undefined
    const page = pages ? Math.min(pages, Math.max(1, r.page)) : undefined
    const location: ReaderLocation = {
      cfi: String(d.cfi ?? ''),
      fraction: Number(d.fraction ?? 0),
      sectionIndex: section.current,
      sectionCount: section.total,
      chapterLabel: (d.tocItem as { label?: string } | undefined)?.label?.trim() ?? '',
      tocHref: (d.tocItem as { href?: string } | undefined)?.href,
      // foliate's `time.section` is in units of 1600 characters.
      sectionCharsLeft: (time?.section ?? 0) * 1600,
      page,
      pages,
      approximate: false,
      reason,
    }
    if (view.isFixedLayout && r) {
      location.fixedPages = r
        .getContents()
        .map((x) => (x.doc ? this.#docIndex.get(x.doc) : undefined))
        .filter((i): i is number => i !== undefined) // a blank half of a spread
        .map((i) => i + 1)
        .sort((a, b) => a - b)
    }
    const c = this.#chunksOf(view)
    const scrolled = this.#mode === 'scroll'
    if (c && ((pages && page) || scrolled)) {
      // L16: foliate measures within the laid-out chunk; place it within the section and book.
      const inChunk = scrolled ? this.#scrollInUnit : pages! > 1 ? (page! - 1) / (pages! - 1) : 0
      const f = sectionFraction(c, c.current, inChunk)
      const totalChars = c.chars.reduce((a, b) => a + b, 0)
      location.sectionCharsLeft = (1 - f) * totalChars
      location.fraction = this.#bookFraction(section.current, f)
      location.approximate = true
      location.sectionFraction = f
    }
    return location
  }

  /** Book fraction of a place `f` (0–1) through linear section `index`, by section size. */
  #bookFraction(index: number, f: number): number {
    const sizes = (this.#book?.sections ?? []).map((s) => (s.linear !== 'no' ? s.size : 0))
    const total = sizes.reduce((a, b) => a + b, 0) || 1
    const before = sizes.slice(0, index).reduce((a, b) => a + b, 0)
    return (before + f * sizes[index]) / total
  }
}
