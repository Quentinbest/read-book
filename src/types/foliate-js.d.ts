// Minimal types for the parts of foliate-js (pinned at 78914ae) that Linen uses.
// The library has no declared stable API (plan §3); keep this surface small and
// behind the ReaderEngine adapter.

declare module 'foliate-js/view.js' {
  export interface TOCItem {
    label: string
    href: string
    subitems?: TOCItem[]
  }

  export interface Section {
    id: string
    linear?: string
    size: number
    cfi: string
    load(): Promise<string> | string
    unload?(): void
    createDocument(): Promise<Document>
  }

  export interface Book {
    sections: Section[]
    toc?: TOCItem[]
    pageList?: TOCItem[]
    landmarks?: { type: string[]; href: string; label?: string }[]
    metadata?: { title?: unknown; language?: string | string[]; identifier?: string }
    rendition?: { layout?: string; spread?: string }
    dir?: string
    transformTarget?: EventTarget
    resolveHref?(href: string): { index: number; anchor(doc: Document): Element | Range | null }
    isExternal?(href: string): boolean
    destroy?(): void
  }

  export interface Location {
    fraction: number
    cfi: string
    range: Range
    tocItem?: { label: string; href: string }
    section?: { current: number; total: number }
  }

  export interface Contents {
    index: number
    doc: Document
    overlayer?: unknown
  }

  export interface Paginator extends HTMLElement {
    setStyles(styles: string | [string, string]): void
    getContents(): Contents[]
    next(distance?: number): Promise<void>
    prev(distance?: number): Promise<void>
    goTo(target: unknown): Promise<void>
    readonly page: number
    readonly pages: number
    readonly size: number
    /** Scroll offset of the current page, in pixels. */
    readonly start: number
    readonly scrolled: boolean
    focusView?(): void
    render(): void
    nextSection(): Promise<void>
    prevSection(): Promise<void>
  }

  export class View extends HTMLElement {
    book: Book
    renderer: Paginator
    lastLocation: Location | null
    isFixedLayout: boolean
    open(book: Book | Blob | File | string): Promise<void>
    close(): void
    init(options: { lastLocation?: string; showTextStart?: boolean }): Promise<void>
    goTo(target: string | number): Promise<unknown>
    goToFraction(fraction: number): Promise<void>
    goToTextStart(): Promise<void>
    getCFI(index: number, range: Range): string
    resolveNavigation(target: string | number | { fraction: number }): unknown
    resolveCFI(cfi: string): { index: number; anchor(doc: Document): Range | Element }
    addAnnotation(annotation: { value: string; color?: string }, remove?: boolean): Promise<unknown>
    deleteAnnotation(annotation: { value: string }): Promise<unknown>
    next(distance?: number): Promise<void>
    prev(distance?: number): Promise<void>
    search(opts: { query: string; index?: number }): AsyncGenerator<unknown>
  }

  export function makeBook(file: Blob | File | string): Promise<Book>
}

declare module 'foliate-js/overlayer.js' {
  export class Overlayer {
    static highlight(rects: DOMRectList | DOMRect[], options?: { color?: string }): SVGElement
    static outline(
      rects: DOMRectList | DOMRect[],
      options?: { color?: string; width?: number },
    ): SVGElement
    readonly element: SVGElement
  }
}

declare module 'foliate-js/epub.js' {
  import type { Book } from 'foliate-js/view.js'
  export class EPUB {
    constructor(loader: {
      loadText(name: string): Promise<string | null>
      loadBlob(name: string, type?: string): Promise<Blob | null>
      getSize(name: string): number
    })
    init(): Promise<Book>
  }
}

declare module 'foliate-js/epubcfi.js' {
  /** Negative, zero or positive as `a` is before, at or after `b`. */
  export function compare(a: string, b: string): number
  export function parse(cfi: string): unknown
  export function toRange(doc: Document, parts: unknown): Range
}
