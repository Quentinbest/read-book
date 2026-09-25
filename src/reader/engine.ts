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
import { MIN_SIDE_MARGIN, type Layout } from './layout'
import type { EntryLoader } from './loader'
import { PARAGRAPH_SPACING_CSS } from './styles'

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

  onLink(l: Listener<LinkEvent>) {
    this.#link.add(l)
    return () => this.#link.delete(l)
  }

  get mode(): ReadingMode {
    return this.#mode
  }

  /**
   * B8, G8: switch between Pages and Scroll, keeping the place. Fixed-layout and
   * vertical books stay in Pages. Returns whether the mode is now `mode`.
   */
  async setMode(mode: ReadingMode): Promise<boolean> {
    if (mode === this.#mode) return true
    if (mode === 'scroll' && (this.fixedLayout || this.#vertical())) return false
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
    if (this.#mode !== 'scroll') return
    this.#lastInputAt = performance.now()
    this.#host.scrollTop += dy
  }

  /** I9: scroll by lines in Scroll mode (↓ ↑). */
  scrollLines(n: number) {
    if (this.#mode !== 'scroll' || !this.#layout) return
    this.#lastInputAt = performance.now()
    this.#host.scrollBy(0, n * this.#layout.lineHeightPx)
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
    this.#current.renderer?.focusView?.()
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
      if (this.#vertical()) await this.setMode('pages') // G8: vertical books read in Pages
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
  applyLayout(layout: Layout, styles: string) {
    this.#layout = layout
    this.#styles = styles
    if (this.#mode === 'scroll') {
      const cfi = this.location?.cfi
      for (const view of this.#views()) if (view.renderer) this.#configureRenderer(view)
      // Heights change with the width and font: lay the stack out again at the same place.
      if (this.#book && cfi) void this.#navigate(cfi)
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

  setStyles(styles: string) {
    this.#styles = styles
    for (const view of this.#views()) if (!view.isFixedLayout) view.renderer?.setStyles?.(styles)
  }

  /** Turn a page. A turn requested while one is running is queued (at most one, I6). */
  turn(dir: Turn): Promise<void> {
    this.#lastInputAt = performance.now()
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
      const href = String((e as CustomEvent<{ href: string }>).detail.href)
      if (/^\s*javascript:/i.test(href)) return
      this.#link.forEach((l) => l({ href, external: false }))
      void this.#navigate(href)
    })
    view.addEventListener('load', (e) => {
      const { doc, index } = (e as CustomEvent<{ doc: Document; index: number }>).detail
      this.#onLoad(view, doc, index)
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
    if (this.#mode === 'scroll' && view !== this.#counter) {
      // B8: one unit at full height; the column is the measure, with no gap of its own.
      r.setAttribute('flow', 'scrolled')
      r.setAttribute('margin', '0px')
      r.setAttribute('gap', '0%')
      r.setAttribute('max-column-count', '1')
      r.setAttribute('max-inline-size', `${layout.columnWidth}px`)
      r.setStyles(this.#styles)
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
    r.setStyles(this.#styles)
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
    const view = this.#current
    const resolved = view.resolveNavigation(target) as { index: number } | undefined
    if (!resolved) return
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
      await this.#scrollToTarget(view, target)
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

  /** Scroll mode after a navigation: rebuild the stack around `view` and bring the target to the top. */
  async #scrollToTarget(view: View, target: string | number | { fraction: number }) {
    this.#stackBusy = true
    try {
      await this.#settled(view)
      const unit = this.#unitOf(view)
      if (!unit) return
      const centre: Slot = { view, unit, top: 0, height: view.renderer.viewSize }
      const others = this.#views().filter((v) => v !== view && v !== this.#counter)
      const before = this.#adjacentUnit(-1, view)
      const after = this.#adjacentUnit(1, view)
      const slots: Slot[] = [centre]
      if (before) slots.unshift(await this.#loadUnit(others[0], before))
      if (after) slots.push(await this.#loadUnit(others[1], after))
      this.#slots = slots
      this.#current = view
      this.#placeSlots(0)
      const y = this.#targetOffset(view, target)
      const layoutTop = this.#layout?.top ?? 88
      this.#host.scrollTop = y < 1 ? centre.top - layoutTop : centre.top + y - SCROLL_FADE_TOP
    } finally {
      this.#stackBusy = false
    }
    this.#emitScrollLocation()
  }

  /** Where a navigation target sits within its view, in px from the unit's top. */
  #targetOffset(view: View, target: string | number | { fraction: number }): number {
    if (typeof target === 'number') return 0
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
      if (!atStart || shift > 0) this.#host.scrollTop += shift
      else this.#host.scrollTop = Math.max(0, this.#host.scrollTop + shift)
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
    const i = Math.max(0, this.#slotIndexAt(this.#host.scrollTop + SCROLL_FADE_TOP))
    this.#placeSlots(i)
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
    this.#checkHeights()
    // The chapter under the middle of the window is the current one (G8 note 3).
    const i = this.#slotIndexAt(this.#host.scrollTop + this.#host.clientHeight / 2)
    this.#current = this.#slots[i].view
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
    if (this.#styles && !view.isFixedLayout) view.renderer?.setStyles?.(this.#styles)
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
