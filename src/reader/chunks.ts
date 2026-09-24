// L16: very long chapters are laid out one chunk at a time.
//
// foliate-js lays out a whole section before it shows a page; a 1.3 MB chapter
// took 1.9 s. A chapter over the threshold is divided at block boundaries into
// chunks of about CHUNK_CHARS characters, and a view lays out only one chunk:
// the other blocks are `display: none`. Nothing is removed from the document, so
// CFIs, highlights and links keep pointing at the same nodes.

/** Sections larger than this (bytes of markup) are chunked (L16: “over ~1 MB”). */
export const CHUNK_THRESHOLD_BYTES = 1_000_000
/** Characters of text per chunk: one chunk lays out in about 120 ms (measured). */
export const CHUNK_CHARS = 60_000

const BLOCKS = 'p, h1, h2, h3, h4, h5, h6, li, blockquote, pre, table, figure, hr, dt, dd, img, svg'
const HIDDEN_ATTR = 'data-linen-hidden'
const HIDE_CSS = `[${HIDDEN_ATTR}] { display: none !important; }`

export interface Chunks {
  /** Outermost block elements in document order. */
  blocks: Element[]
  /** Index of each block. */
  indexOf: Map<Element, number>
  /** starts[k] = index of the first block of chunk k; starts[count] = blocks.length. */
  starts: number[]
  /** Characters of text in each chunk. */
  chars: number[]
  /** Elements that hold blocks without being blocks (e.g. `<section>`), with their first and last block. */
  containers: [Element, number, number][]
  /** The chunk currently laid out. */
  current: number
}

export const chunkCount = (c: Chunks) => c.starts.length - 1

/** Divide a document's blocks into chunks of about `size` characters. */
export function computeChunks(doc: Document, size = CHUNK_CHARS): Chunks {
  const blocks = Array.from(doc.body?.querySelectorAll(BLOCKS) ?? []).filter(
    (el) => !el.parentElement?.closest(BLOCKS),
  )
  const starts = [0]
  const chars: number[] = []
  let acc = 0
  blocks.forEach((el, i) => {
    const n = el.textContent?.length ?? 0
    if (acc > 0 && acc + n > size) {
      starts.push(i)
      chars.push(acc)
      acc = 0
    }
    acc += n
  })
  starts.push(blocks.length)
  chars.push(acc)
  // An empty wrapper still takes a place in the layout; foliate would count it as
  // on the page. Wrappers whose blocks are all outside the chunk are hidden too.
  const spans = new Map<Element, [number, number]>()
  blocks.forEach((el, i) => {
    for (let a = el.parentElement; a && a !== doc.body; a = a.parentElement) {
      const span = spans.get(a)
      if (span) span[1] = i
      else spans.set(a, [i, i])
    }
  })
  const containers = Array.from(spans, ([el, [a, b]]) => [el, a, b] as [Element, number, number])
  return {
    blocks,
    indexOf: new Map(blocks.map((b, i) => [b, i])),
    starts,
    chars,
    containers,
    current: -1,
  }
}

/** Lay out chunk `k` only: every block outside it is hidden. */
export function showChunk(doc: Document, c: Chunks, k: number) {
  if (!doc.getElementById('linen-chunk-style')) {
    const style = doc.createElement('style')
    style.id = 'linen-chunk-style'
    style.textContent = HIDE_CSS
    ;(doc.head ?? doc.documentElement).append(style)
  }
  const [from, to] = [c.starts[k], c.starts[k + 1]]
  c.blocks.forEach((el, i) => {
    const hide = i < from || i >= to
    if (hide !== el.hasAttribute(HIDDEN_ATTR)) el.toggleAttribute(HIDDEN_ATTR, hide)
  })
  for (const [el, first, last] of c.containers) {
    const hide = last < from || first >= to
    if (hide !== el.hasAttribute(HIDDEN_ATTR)) el.toggleAttribute(HIDDEN_ATTR, hide)
  }
  c.current = k
}

/** The chunk holding `node` (the chunk of the nearest block at or before it). */
export function chunkOf(c: Chunks, node: Node | null): number {
  let el: Element | null = node instanceof Element ? node : (node?.parentElement ?? null)
  let block: Element | null = null
  while (el) {
    if (c.indexOf.has(el)) block = el
    el = el.parentElement
  }
  let index = block ? c.indexOf.get(block)! : -1
  if (index < 0 && node) {
    // Not inside a block (e.g. a section wrapper): use the first block after it.
    index = c.blocks.findIndex(
      (b) => node.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING,
    )
    if (index < 0) index = c.blocks.length - 1
  }
  for (let k = 0; k < chunkCount(c); k++) if (index < c.starts[k + 1]) return k
  return chunkCount(c) - 1
}

/** Position within the section (0–1) of a fraction of the way through chunk `k`. */
export function sectionFraction(c: Chunks, k: number, fractionInChunk: number): number {
  const total = c.chars.reduce((a, b) => a + b, 0) || 1
  const before = c.chars.slice(0, k).reduce((a, b) => a + b, 0)
  return (before + fractionInChunk * c.chars[k]) / total
}
