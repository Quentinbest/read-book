// Footnotes (N9, E1). Note references are recognised by their EPUB 3 or ARIA
// semantics; the note itself is copied into the app's peek as plain structure
// (paragraphs, emphasis, sub- and superscripts) and nothing else: book markup is
// hostile-capable, so no attributes, styles, links or scripts survive (§7).

const OPS = 'http://www.idpf.org/2007/ops'

function semantics(el: Element): string {
  const epubType =
    el.getAttributeNS(OPS, 'type') ?? el.getAttribute('epub:type') ?? el.getAttribute('type') ?? ''
  return `${epubType} ${el.getAttribute('role') ?? ''}`
}

/** A note reference: `epub:type="noteref"` or `role="doc-noteref"`. */
export function isNoteRef(el: Element | null): el is Element {
  return !!el && /\b(noteref|doc-noteref)\b/.test(semantics(el))
}

/** A note body: footnote, endnote or rearnote, by EPUB 3 or ARIA semantics. */
export function isNote(el: Element): boolean {
  return /\b(footnote|endnote|rearnote|note|doc-footnote|doc-endnote)\b/.test(semantics(el))
}

/** The element to show for a note target: the note container around it, else the target. */
export function noteContainer(target: Element): Element {
  for (let e: Element | null = target; e; e = e.parentElement) if (isNote(e)) return e
  // Endnote lists often mark only the list item's link target.
  // A list item or aside can hold several paragraphs, so it wins over the paragraph.
  return target.closest('li, aside') ?? target.closest('p') ?? target
}

/** Footnote asides that a note reference in the same document points at (hidden from the flow). */
export function referencedFootnoteAsides(doc: Document): Element[] {
  const ids = new Set(
    Array.from(doc.querySelectorAll('a[href^="#"]'))
      .filter(isNoteRef)
      .map((a) => decodeURIComponent(a.getAttribute('href')!.slice(1))),
  )
  return Array.from(doc.querySelectorAll('aside')).filter(
    (el) => el.id && ids.has(el.id) && /\b(footnote|doc-footnote)\b/.test(semantics(el)),
  )
}

const BLOCK = new Set(['p', 'div', 'li', 'blockquote', 'aside', 'section', 'dd', 'ol', 'ul'])
const INLINE: Record<string, string> = {
  em: 'em',
  i: 'em',
  cite: 'cite',
  strong: 'strong',
  b: 'strong',
  sup: 'sup',
  sub: 'sub',
  small: 'small',
  q: 'q',
  code: 'code',
}
const DROP = new Set([
  'script',
  'style',
  'template',
  'iframe',
  'object',
  'embed',
  'svg',
  'math',
  'img',
])

/**
 * Copy a note into `into` as safe structure: text, paragraphs and a few inline
 * emphasis tags, with no attributes. Back-links (“↩”) inside the note are dropped.
 */
export function copyNote(from: Element, into: DocumentFragment | HTMLElement) {
  const doc = into.ownerDocument
  const walk = (node: Node, out: Node) => {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === 3) {
        out.appendChild(doc.createTextNode(child.textContent ?? ''))
        continue
      }
      if (child.nodeType !== 1) continue
      const el = child as Element
      const tag = el.localName.toLowerCase()
      if (DROP.has(tag)) continue
      // The note's own link back to its reference.
      if (tag === 'a' && /\b(backlink|doc-backlink)\b/.test(semantics(el))) continue
      if (tag === 'br') {
        out.appendChild(doc.createElement('br'))
      } else if (BLOCK.has(tag)) {
        const p = doc.createElement('p')
        walk(el, p)
        if (p.textContent?.trim()) out.appendChild(p)
      } else if (INLINE[tag]) {
        const e = doc.createElement(INLINE[tag])
        walk(el, e)
        out.appendChild(e)
      } else walk(el, out) // unknown or <a>: keep the text, drop the element
    }
  }
  walk(from, into)
  // A note with only inline content still reads as a paragraph.
  if (!Array.from(into.childNodes).some((n) => n.nodeName === 'P')) {
    const p = doc.createElement('p')
    while (into.firstChild) p.appendChild(into.firstChild)
    into.appendChild(p)
  }
}

/** Plain text for Copy. */
export function noteText(from: Element): string {
  const box = document.createElement('div')
  copyNote(from, box)
  return Array.from(box.querySelectorAll('p'))
    .map((p) => p.textContent?.replace(/\s+/g, ' ').trim() ?? '')
    .filter(Boolean)
    .join('\n\n')
}
