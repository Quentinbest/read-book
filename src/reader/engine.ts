// ReaderEngine: the adapter around foliate-js (plan §3; decisions D-E1, D-D1, D-X1).
//
// foliate-js has no stable API, so everything the app needs from it goes
// through here: opening, layout, page turns, locations, links and the content
// hooks. Spike findings built in:
// - content passes through `transformContent` (per-document CSP, CSS sanitiser);
// - every foliate link event is cancelled and routed by the app (N10);
// - one column below the spread breakpoint (L8);
// - page turns queue at most one pending turn (I6) instead of being dropped;
// - D-D1: the previous and next sections are laid out ahead in hidden views, so
//   a turn across a chapter boundary is a swap, not a load (Spike D: loading in
//   the turn took p95 27–57 ms against a 16 ms budget);
// - scrolls the engine did not make (VoiceOver, X3) are detected, snapped to a
//   whole page and reported as a location change.

import 'foliate-js/view.js'
import type { Book, View } from 'foliate-js/view.js'
import { MIN_SIDE_MARGIN, PAGINATOR_GAP, type Layout } from './layout'
import { transformContent } from './content'
import { PARAGRAPH_SPACING_CSS } from './styles'

export interface ReaderLocation {
  cfi: string
  /** Position in the whole book, 0–1. */
  fraction: number
  sectionIndex: number
  sectionCount: number
  /** Label of the current table-of-contents entry. */
  chapterLabel: string
  /** Characters left in this section (B2 turns them into minutes). */
  sectionCharsLeft: number
  /** Page within the section (1-based) and the section's page count, when laid out. */
  page?: number
  pages?: number
  reason: 'page' | 'navigation' | 'scroll' | 'selection' | 'anchor' | 'snap' | 'external'
}

export type Turn = 'next' | 'prev'

export interface LinkEvent {
  href: string
  external: boolean
}

type Listener<T> = (value: T) => void

/** A hidden view parked at a neighbouring section, ready to be swapped in. */
interface Neighbour {
  view: View
  /** The section it shows, or -1 while loading / unused. */
  index: number
  ready: Promise<void>
}

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
  #turning: Promise<void> | null = null
  #pending: Turn | null = null
  #styles = ''
  #layout: Layout | null = null
  #lastPage = -1
  #watchTimer = 0
  /** Recent engine events, for diagnosing view swaps. */
  #trail: string[] = []
  #note(e: string) {
    this.#trail.push(
      `${Math.round(performance.now())} ${e} views=${this.#views()
        .map((v) => this.#indexOf(v))
        .join(',')}`,
    )
    if (this.#trail.length > 30) this.#trail.shift()
  }

  constructor(host: HTMLElement) {
    this.#host = host
    this.#current = this.#createView()
    this.#next = { view: this.#createView(), index: -1, ready: Promise.resolve() }
    this.#prev = { view: this.#createView(), index: -1, ready: Promise.resolve() }
    this.#show(this.#current)
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

  /** Focus the page (the book text), e.g. when a layer closes (S5). */
  focusPage() {
    this.#current.renderer?.focusView?.()
  }

  async open(file: File, start?: { cfi?: string }): Promise<Book> {
    const { makeBook } = await import('foliate-js/view.js')
    const book = await makeBook(file)
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
    for (const view of this.#views()) if (view.renderer) this.#configureRenderer(view)
    // N3: new books open at the bodymatter landmark, not the cover.
    await this.#current.init(start?.cfi ? { lastLocation: start.cfi } : { showTextStart: true })
    this.#prepareNeighbours()
    return book
  }

  /** Place the page box and tell the paginators their geometry (L1–L3, L8). */
  applyLayout(layout: Layout, styles: string) {
    this.#layout = layout
    this.#styles = styles
    for (const view of this.#views()) {
      // Fixed-layout pages fill the reading area inside the minimum margins (E2; G8 pending).
      const fixed = view.isFixedLayout === true
      Object.assign(view.style, {
        left: `${fixed ? MIN_SIDE_MARGIN : layout.viewLeft}px`,
        top: `${layout.top}px`,
        width: fixed ? `calc(100% - ${2 * MIN_SIDE_MARGIN}px)` : `${layout.viewWidth}px`,
        height: `${layout.pageHeight}px`,
      })
      // foliate-js creates its paginator in open(); until then the layout waits.
      if (view.renderer) this.#configureRenderer(view)
    }
    // Reflow moves the neighbours' last and first pages: park them again.
    if (this.#book) this.#prepareNeighbours(true)
  }

  setStyles(styles: string) {
    this.#styles = styles
    for (const view of this.#views()) if (!view.isFixedLayout) view.renderer?.setStyles?.(styles)
  }

  /** Turn a page. A turn requested while one is running is queued (at most one, I6). */
  turn(dir: Turn): Promise<void> {
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

  async goTo(target: string | number) {
    this.#note(`goTo ${target}`)
    const result = await this.#current.goTo(target)
    this.#prepareNeighbours()
    return result
  }

  async goToFraction(fraction: number) {
    await this.#current.goToFraction(fraction)
    this.#prepareNeighbours()
  }

  async goToTextStart() {
    await this.#current.goToTextStart()
    this.#prepareNeighbours()
  }

  async nextSection() {
    await this.#current.renderer.nextSection()
    this.#prepareNeighbours()
  }

  async prevSection() {
    await this.#current.renderer.prevSection()
    this.#prepareNeighbours()
  }

  get location(): ReaderLocation | null {
    const l = this.#current.lastLocation
    return l
      ? this.#toLocation(this.#current, l as unknown as Record<string, unknown>, 'navigation')
      : null
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
      views: this.#views().map((v) => ({
        index: this.#indexOf(v),
        renderer: !!v.renderer,
        connected: v.isConnected,
      })),
      linear: (this.#book?.sections ?? []).map((s) => s.linear ?? ''),
      next: this.#next.index,
      prev: this.#prev.index,
    }
  }

  // ---------------------------------------------------------------- internals

  #views(): View[] {
    return [this.#current, this.#next.view, this.#prev.view]
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
      if (view === this.#current) this.#onRelocate(view, (e as CustomEvent).detail)
    })
    view.addEventListener('external-link', (e) => {
      e.preventDefault()
      const href = String((e as CustomEvent<{ href_: string }>).detail.href_)
      this.#link.forEach((l) => l({ href, external: true }))
    })
    view.addEventListener('link', (e) => {
      // Internal links navigate, but the app hears about them first (N1, N10).
      const href = String((e as CustomEvent<{ href: string }>).detail.href)
      if (/^\s*javascript:/i.test(href)) {
        e.preventDefault()
        return
      }
      this.#link.forEach((l) => l({ href, external: false }))
      // The jump happens in the visible view; the neighbours follow afterwards.
      queueMicrotask(() => setTimeout(() => this.#prepareNeighbours(), 300))
    })
    view.addEventListener('load', (e) =>
      this.#onLoad(view, (e as CustomEvent<{ doc: Document }>).detail.doc),
    )
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
    r.setAttribute('flow', 'paginated')
    r.setAttribute('margin', '0px')
    // Text width = view width × (1 − 2 × gap): see PAGINATOR_GAP in layout.ts.
    r.setAttribute('gap', `${PAGINATOR_GAP * 100}%`)
    r.setAttribute('max-block-size', `${Math.ceil(layout.pageHeight)}px`)
    r.setAttribute('max-column-count', String(layout.columns))
    // One column fills the view; two columns split it (see layout.ts).
    r.setAttribute(
      'max-inline-size',
      `${layout.columns === 1 ? 100000 : Math.ceil(layout.viewWidth / 2)}px`,
    )
    r.setStyles(this.#styles)
  }

  #currentIndex(): number {
    return this.#current.renderer?.getContents()[0]?.index ?? -1
  }

  /** The next linear section after `index` in `dir` (foliate skips linear="no"), or -1. */
  #adjacent(index: number, dir: 1 | -1): number {
    const sections = this.#book?.sections ?? []
    for (let i = index + dir; i >= 0 && i < sections.length; i += dir)
      if (sections[i].linear !== 'no') return i
    return -1
  }

  /** Park the hidden views at the first page of the next section and the last page of the previous one. */
  #prepareNeighbours(force = false) {
    if (this.fixedLayout) return
    const index = this.#currentIndex()
    if (index < 0) return
    const park = (n: Neighbour, target: number, anchor: number) => {
      if (target < 0) {
        n.index = -1
        return
      }
      if (!force && n.index === target && this.#indexOf(n.view) === target) return
      n.index = -1
      const view = n.view
      this.#note(`park ${n === this.#next ? 'next' : 'prev'} -> ${target}`)
      // foliate-js ignores goTo while a view is locked after a turn, and still
      // resolves: record what the view actually shows, not what was asked.
      n.ready = view.renderer
        .goTo({ index: target, anchor })
        .then(() => {
          if (n.view === view) n.index = this.#indexOf(view)
        })
        .catch(() => {
          if (n.view === view) n.index = -1
        })
    }
    park(this.#next, this.#adjacent(index, 1), 0)
    park(this.#prev, this.#adjacent(index, -1), 1)
  }

  async #turnNow(dir: Turn) {
    if (this.fixedLayout) {
      await (dir === 'next' ? this.#current.next() : this.#current.prev())
      return
    }
    const r = this.#current.renderer
    if (!r?.getContents().length) {
      this.#note('turn ignored: the visible view shows nothing')
      return
    }
    const crossing = dir === 'next' ? r.page >= r.pages - 2 : r.page <= 1
    const target = this.#adjacent(this.#currentIndex(), dir === 'next' ? 1 : -1)
    const neighbour = dir === 'next' ? this.#next : this.#prev
    if (
      crossing &&
      target >= 0 &&
      neighbour.index === target &&
      this.#indexOf(neighbour.view) === target
    ) {
      // D-D1: the neighbour is laid out at the right page; swapping is the turn.
      this.#note(`swap ${dir} to ${target}`)
      const old = this.#current
      this.#current = neighbour.view
      this.#show(this.#current)
      // The old view becomes the neighbour on the other side, already at the right page.
      const other = dir === 'next' ? this.#prev : this.#next
      const recycled = other.view
      other.view = old
      other.index =
        this.#adjacent(target, dir === 'next' ? -1 : 1) === this.#indexOf(old)
          ? this.#indexOf(old)
          : -1
      neighbour.view = recycled
      neighbour.index = -1
      const l = this.#current.lastLocation
      if (l) this.#onRelocate(this.#current, l as unknown as Record<string, unknown>, 'page')
      // Load the new far neighbour after this frame, off the turn.
      requestAnimationFrame(() => this.#prepareNeighbours())
      return
    }
    this.#note(`turn ${dir} crossing=${crossing}`)
    await (dir === 'next' ? this.#current.next() : this.#current.prev())
    if (crossing) this.#prepareNeighbours()
  }

  #indexOf(view: View): number {
    return view.renderer?.getContents()[0]?.index ?? -1
  }

  #onLoad(view: View, doc: Document) {
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
    return {
      cfi: String(d.cfi ?? ''),
      fraction: Number(d.fraction ?? 0),
      sectionIndex: section.current,
      sectionCount: section.total,
      chapterLabel: (d.tocItem as { label?: string } | undefined)?.label?.trim() ?? '',
      // foliate's `time.section` is in units of 1600 characters.
      sectionCharsLeft: (time?.section ?? 0) * 1600,
      page: pages ? Math.min(pages, Math.max(1, r.page)) : undefined,
      pages,
      reason,
    }
  }
}
