// Book-wide page numbers at the current layout (decision B1).
//
// Pages of sections that have been laid out are counted; the rest are estimated
// from their size at the characters-per-page seen so far. Numbers are marked
// approximate until every section has been counted (L16).

export class PageCounter {
  #sizes: number[]
  #counted = new Map<number, number>()

  constructor(sectionSizes: number[]) {
    this.#sizes = sectionSizes.map((s) => Math.max(0, s))
  }

  /** Forget counts after a reflow (font, spacing or window change). */
  reset() {
    this.#counted.clear()
  }

  count(section: number, pages: number) {
    if (pages > 0) this.#counted.set(section, pages)
  }

  get exact(): boolean {
    return this.#sizes.every((size, i) => size === 0 || this.#counted.has(i))
  }

  #charsPerPage(): number {
    let chars = 0
    let pages = 0
    for (const [i, p] of this.#counted) {
      chars += this.#sizes[i]
      pages += p
    }
    return pages > 0 && chars > 0 ? chars / pages : 2000
  }

  #pagesOf(i: number): number {
    return (
      this.#counted.get(i) ??
      Math.max(this.#sizes[i] > 0 ? 1 : 0, Math.round(this.#sizes[i] / this.#charsPerPage()))
    )
  }

  get total(): number {
    return this.#sizes.reduce((sum, _s, i) => sum + this.#pagesOf(i), 0)
  }

  /** 1-based page number of `page` (1-based) within `section`. */
  pageNumber(section: number, page: number): number {
    let before = 0
    for (let i = 0; i < section; i++) before += this.#pagesOf(i)
    return before + Math.max(1, page)
  }
}
