// Annotations (A4–A9): a highlight in one of four colours, maybe with a note,
// anchored by a CFI and a text quote. Serialised as W3C Web Annotations (A9),
// which export and the extension API use; the round trip is lossless.

import type { TextQuote } from './anchor'

export type HighlightColor = 'yellow' | 'green' | 'blue' | 'rose'
export const COLORS: HighlightColor[] = ['yellow', 'green', 'blue', 'rose']

/** 'anchored'; 'reanchor' after the file was replaced; 'unplaced' when it could not be placed (A8). */
export type AnchorStatus = 'anchored' | 'reanchor' | 'unplaced'

export interface Annotation {
  id: string
  bookId: string
  /** The book file's content hash when the anchor was made (B3). */
  anchoredContentHash: string
  color: HighlightColor
  cfi: string
  quote: TextQuote
  note: string | null
  createdAt: number
  updatedAt: number
  status: AnchorStatus
}

/** The row the core stores (snake case, see `store::AnnotationRow`). */
export interface AnnotationRow {
  id: string
  book_id: string
  anchored_content_hash: string
  color: string
  cfi_range: string
  quote_exact: string
  quote_prefix: string
  quote_suffix: string
  note: string | null
  created_at: number
  updated_at: number
  anchor_status: string
}

export function fromRow(r: AnnotationRow): Annotation {
  return {
    id: r.id,
    bookId: r.book_id,
    anchoredContentHash: r.anchored_content_hash,
    color: (COLORS as string[]).includes(r.color) ? (r.color as HighlightColor) : 'yellow',
    cfi: r.cfi_range,
    quote: { exact: r.quote_exact, prefix: r.quote_prefix, suffix: r.quote_suffix },
    note: r.note,
    createdAt: r.created_at,
    updatedAt: r.updated_at,
    status: (['anchored', 'reanchor', 'unplaced'] as string[]).includes(r.anchor_status)
      ? (r.anchor_status as AnchorStatus)
      : 'anchored',
  }
}

export function toRow(a: Annotation): AnnotationRow {
  return {
    id: a.id,
    book_id: a.bookId,
    anchored_content_hash: a.anchoredContentHash,
    color: a.color,
    cfi_range: a.cfi,
    quote_exact: a.quote.exact,
    quote_prefix: a.quote.prefix,
    quote_suffix: a.quote.suffix,
    note: a.note,
    created_at: a.createdAt,
    updated_at: a.updatedAt,
    anchor_status: a.status,
  }
}

// ---------------------------------------------------------------- W3C Web Annotation

const LINEN_NS = 'https://linen.app/ns/annotation#'
const CFI_SPEC = 'http://www.idpf.org/epub/linking/cfi/epub-cfi.html'

export interface W3CAnnotation {
  '@context': [string, { linen: string }]
  id: string
  type: 'Annotation'
  motivation: 'highlighting' | 'commenting'
  created: string
  modified: string
  body: { type: 'TextualBody'; value: string; format: 'text/plain'; purpose: 'commenting' }[]
  target: {
    source: string
    selector: (
      | { type: 'FragmentSelector'; conformsTo: string; value: string }
      | { type: 'TextQuoteSelector'; exact: string; prefix: string; suffix: string }
    )[]
  }
  'linen:color': HighlightColor
  'linen:anchorStatus': AnchorStatus
  'linen:anchoredContentHash': string
}

/** `source` identifies the book: its package identifier, else Linen's own id. */
export function toW3C(a: Annotation, source: string): W3CAnnotation {
  return {
    '@context': ['http://www.w3.org/ns/anno.jsonld', { linen: LINEN_NS }],
    id: `urn:uuid:${a.id}`,
    type: 'Annotation',
    motivation: a.note ? 'commenting' : 'highlighting',
    created: new Date(a.createdAt).toISOString(),
    modified: new Date(a.updatedAt).toISOString(),
    body: a.note
      ? [{ type: 'TextualBody', value: a.note, format: 'text/plain', purpose: 'commenting' }]
      : [],
    target: {
      source,
      selector: [
        { type: 'FragmentSelector', conformsTo: CFI_SPEC, value: a.cfi },
        { type: 'TextQuoteSelector', ...a.quote },
      ],
    },
    'linen:color': a.color,
    'linen:anchorStatus': a.status,
    'linen:anchoredContentHash': a.anchoredContentHash,
  }
}

export function fromW3C(w: W3CAnnotation, bookId: string): Annotation {
  const cfi = w.target.selector.find((s) => s.type === 'FragmentSelector')
  const quote = w.target.selector.find((s) => s.type === 'TextQuoteSelector')
  return {
    id: w.id.replace(/^urn:uuid:/, ''),
    bookId,
    anchoredContentHash: w['linen:anchoredContentHash'],
    color: w['linen:color'],
    cfi: cfi && 'value' in cfi ? cfi.value : '',
    quote:
      quote && 'exact' in quote
        ? { exact: quote.exact, prefix: quote.prefix, suffix: quote.suffix }
        : { exact: '', prefix: '', suffix: '' },
    note: w.body[0]?.value ?? null,
    createdAt: Date.parse(w.created),
    updatedAt: Date.parse(w.modified),
    status: w['linen:anchorStatus'],
  }
}
