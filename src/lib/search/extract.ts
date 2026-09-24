// Plain-text extraction of a chapter for search (F8), with a map back to the DOM
// so a match can be turned into a Range for marks in the page (F5).

/** Elements whose text is not reading text: ruby annotations, scripts, styles. */
const SKIP = new Set(['rt', 'rp', 'script', 'style', 'noscript', 'template', 'head'])

/** Elements that break the text flow; a newline separates them from neighbours. */
const BLOCK = new Set([
  'address',
  'article',
  'aside',
  'blockquote',
  'body',
  'br',
  'caption',
  'dd',
  'div',
  'dl',
  'dt',
  'figcaption',
  'figure',
  'footer',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'header',
  'hr',
  'li',
  'main',
  'nav',
  'ol',
  'p',
  'pre',
  'section',
  'table',
  'td',
  'th',
  'tr',
  'ul',
])

interface Segment {
  /** Offset of this text node's first character in the extracted text. */
  start: number
  node: Text
}

export interface ExtractedText {
  text: string
  /** Text nodes in document order with their starting offsets. */
  segments: Segment[]
}

export function extractText(root: Node): ExtractedText {
  let text = ''
  const segments: Segment[] = []
  const breakLine = () => {
    if (text && !text.endsWith('\n')) text += '\n'
  }
  const walk = (node: Node) => {
    if (node.nodeType === 3) {
      const t = node as Text
      if (t.data) {
        segments.push({ start: text.length, node: t })
        text += t.data
      }
      return
    }
    if (node.nodeType !== 1 && node.nodeType !== 9 && node.nodeType !== 11) return
    const name = node.nodeType === 1 ? (node as Element).localName.toLowerCase() : ''
    if (SKIP.has(name)) return
    const block = BLOCK.has(name)
    if (block) breakLine()
    for (let child = node.firstChild; child; child = child.nextSibling) walk(child)
    if (block) breakLine()
  }
  walk(root)
  return { text, segments }
}

/**
 * The DOM position of an offset in the extracted text, or null if it falls on a
 * separator. At a boundary between two text nodes, `bias` picks the node after
 * it ('start', for the start of a range) or before it ('end').
 */
export function positionAt(
  extracted: ExtractedText,
  offset: number,
  bias: 'start' | 'end' = 'start',
): { node: Text; offset: number } | null {
  const { segments } = extracted
  let found = -1
  let lo = 0
  let hi = segments.length - 1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const seg = segments[mid]
    if (offset < seg.start) hi = mid - 1
    else if (offset > seg.start + seg.node.data.length) lo = mid + 1
    else {
      found = mid
      break
    }
  }
  if (found < 0) return null
  const seg = segments[found]
  const next = segments[found + 1]
  const prev = segments[found - 1]
  if (bias === 'start' && offset === seg.start + seg.node.data.length && next?.start === offset)
    return { node: next.node, offset: 0 }
  if (
    bias === 'end' &&
    offset === seg.start &&
    prev &&
    prev.start + prev.node.data.length === offset
  )
    return { node: prev.node, offset: prev.node.data.length }
  return { node: seg.node, offset: offset - seg.start }
}

/** A DOM Range for [start, end) in the extracted text. */
export function rangeFor(extracted: ExtractedText, start: number, end: number): Range | null {
  const a = positionAt(extracted, start, 'start')
  const b = positionAt(extracted, end, 'end')
  if (!a || !b) return null
  const range = a.node.ownerDocument!.createRange()
  range.setStart(a.node, a.offset)
  range.setEnd(b.node, b.offset)
  return range
}
