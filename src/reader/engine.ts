// ReaderEngine: the adapter around foliate-js (plan §3; decisions D-E1, D-D1, D-X1).
//
// foliate-js has no stable API, so everything the app needs from it goes
// through here: opening, layout, page turns, locations, links and the content
// hooks. Spike findings built in:
// - content passes through `transformContent` (per-document CSP, CSS sanitiser);
// - every foliate link event is cancelled and routed by the app (N10);
// - one column below the spread breakpoint (L8);
// - page turns queue at most one pending turn (I6) instead of being dropped;
// - scrolls the engine did not make (VoiceOver, X3) are detected, snapped to a
//   whole page and reported as a location change.

import 'foliate-js/view.js'
import type { Book, View } from 'foliate-js/view.js'
import { PAGINATOR_GAP, type Layout } from './layout'
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
  /** Minutes left in this section at foliate's default pace (B2 replaces the pace). */
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

export class ReaderEngine {
  readonly view: View
  #host: HTMLElement
  #book: Book | null = null
  #relocate = new Set<Listener<ReaderLocation>>()
  #link = new Set<Listener<LinkEvent>>()
  #key = new Set<Listener<KeyboardEvent>>()
  #turning: Promise<void> | null = null
  #pending: Turn | null = null
  #styles = ''
  #layout: Layout | null = null
  #lastPage = -1
  #watchTimer = 0

  constructor(host: HTMLElement) {
    this.#host = host
    this.view = document.createElement('foliate-view') as View
    Object.assign(this.view.style, { position: 'absolute', display: 'block' })
    // S12: foliate-js hides the pointer over the page after inactivity.
    this.view.setAttribute('autohide-cursor', '')
    host.append(this.view)
    this.view.addEventListener('relocate', (e) => this.#onRelocate((e as CustomEvent).detail))
    this.view.addEventListener('external-link', (e) => {
      e.preventDefault()
      const href = String((e as CustomEvent<{ href_: string }>).detail.href_)
      this.#link.forEach((l) => l({ href, external: true }))
    })
    this.view.addEventListener('link', (e) => {
      // Internal links are allowed to navigate, but the app hears about them first (N1, N10).
      const href = String((e as CustomEvent<{ href: string }>).detail.href)
      if (/^\s*javascript:/i.test(href)) {
        e.preventDefault()
        return
      }
      this.#link.forEach((l) => l({ href, external: false }))
    })
    this.view.addEventListener('load', (e) =>
      this.#onLoad((e as CustomEvent<{ doc: Document }>).detail.doc),
    )
  }

  get book(): Book | null {
    return this.#book
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

  /** Focus the page (the book text), e.g. when a layer closes (S5). */
  focusPage() {
    this.view.renderer?.focusView?.()
  }

  onLink(l: Listener<LinkEvent>) {
    this.#link.add(l)
    return () => this.#link.delete(l)
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
    await this.view.open(book)
    this.#book = book
    this.#configureRenderer()
    // N3: new books open at the bodymatter landmark, not the cover.
    await this.view.init(start?.cfi ? { lastLocation: start.cfi } : { showTextStart: true })
    return book
  }

  /** Place the page box and tell the paginator its geometry (L1–L3, L8). */
  applyLayout(layout: Layout, styles: string) {
    this.#layout = layout
    this.#styles = styles
    Object.assign(this.view.style, {
      left: `${layout.viewLeft}px`,
      top: `${layout.top}px`,
      width: `${layout.viewWidth}px`,
      height: `${layout.pageHeight}px`,
    })
    // foliate-js creates its paginator in open(); until then the layout waits.
    if (this.view.renderer) this.#configureRenderer()
  }

  #configureRenderer() {
    const layout = this.#layout
    const r = this.view.renderer
    if (!layout || !r) return
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

  setStyles(styles: string) {
    this.#styles = styles
    this.view.renderer?.setStyles?.(styles)
  }

  /** Turn a page. A turn requested while one is running is queued (at most one, I6). */
  turn(dir: Turn): Promise<void> {
    if (this.#turning) {
      this.#pending = dir
      return this.#turning
    }
    this.#turning = (dir === 'next' ? this.view.next() : this.view.prev()).finally(() => {
      this.#turning = null
      const pending = this.#pending
      this.#pending = null
      if (pending) void this.turn(pending)
    })
    return this.#turning
  }

  goTo(target: string | number) {
    return this.view.goTo(target)
  }

  goToFraction(fraction: number) {
    return this.view.goToFraction(fraction)
  }

  get location(): ReaderLocation | null {
    const l = this.view.lastLocation
    return l ? this.#toLocation(l as unknown as Record<string, unknown>, 'navigation') : null
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
      const r = this.view.renderer
      const page = r?.page ?? -1
      if (this.#lastPage !== -1 && page !== this.#lastPage && !this.#turning) {
        // Snap to the whole page foliate is now on, then report the location.
        void r.goTo({
          index: this.view.renderer.getContents()[0]?.index,
          anchor: (page - 1) / Math.max(1, r.pages - 2),
        })
        const l = this.view.lastLocation
        if (l)
          this.#relocate.forEach((f) =>
            f(this.#toLocation(l as unknown as Record<string, unknown>, 'external')),
          )
      }
      this.#lastPage = page
      this.#watchTimer = requestAnimationFrame(tick)
    }
    this.#watchTimer = requestAnimationFrame(tick)
  }

  close() {
    cancelAnimationFrame(this.#watchTimer)
    this.view.close()
    this.#book?.destroy?.()
    this.view.remove()
    this.#host.replaceChildren()
  }

  #onLoad(doc: Document) {
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
    if (this.#styles) this.view.renderer?.setStyles?.(this.#styles)
  }

  #onRelocate(detail: Record<string, unknown>) {
    this.#lastPage = this.view.renderer?.page ?? -1
    const loc = this.#toLocation(detail, (detail.reason as ReaderLocation['reason']) ?? 'page')
    this.#relocate.forEach((l) => l(loc))
  }

  #toLocation(d: Record<string, unknown>, reason: ReaderLocation['reason']): ReaderLocation {
    const section = (d.section as { current: number; total: number } | undefined) ?? {
      current: 0,
      total: 1,
    }
    const time = d.time as { section?: number } | undefined
    const r = this.view.renderer
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
