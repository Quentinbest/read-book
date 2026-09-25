import { describe, expect, it } from 'vitest'
import { fromRow, fromW3C, toRow, toW3C, type Annotation } from './model'

const a: Annotation = {
  id: '6f1c2c1e-6d3a-4f0e-9a8b-1c2d3e4f5a6b',
  bookId: 'b1',
  anchoredContentHash: 'sha256-abc',
  color: 'green',
  cfi: 'epubcfi(/6/14!/4/2/8,/1:0,/1:98)',
  quote: {
    exact: 'There now is your insular city of the Manhattoes',
    prefix: 'feelings towards the ocean with me.\n',
    suffix: ', belted round by wharves',
  },
  note: 'Ishmael frames the voyage as a cure — “the sea as medicine”.',
  createdAt: 1_758_700_000_123,
  updatedAt: 1_758_700_100_456,
  status: 'anchored',
}

describe('A9 W3C Web Annotation serialisation', () => {
  it('round-trips without loss, through JSON', () => {
    const json = JSON.stringify(toW3C(a, 'urn:uuid:book-identifier'))
    expect(fromW3C(JSON.parse(json), 'b1')).toEqual(a)
  })

  it('round-trips a highlight without a note, and other statuses', () => {
    const plain: Annotation = { ...a, note: null, color: 'rose', status: 'unplaced' }
    expect(fromW3C(toW3C(plain, 'x'), 'b1')).toEqual(plain)
    expect(toW3C(plain, 'x').motivation).toBe('highlighting')
    expect(toW3C(a, 'x').motivation).toBe('commenting')
  })

  it('uses the standard selectors: an EPUB CFI and a text quote', () => {
    const w = toW3C(a, 'urn:isbn:9780000000000')
    expect(w.target.selector).toEqual([
      {
        type: 'FragmentSelector',
        conformsTo: 'http://www.idpf.org/epub/linking/cfi/epub-cfi.html',
        value: a.cfi,
      },
      { type: 'TextQuoteSelector', ...a.quote },
    ])
    expect(w['@context'][0]).toBe('http://www.w3.org/ns/anno.jsonld')
  })

  it('round-trips the stored row', () => {
    expect(fromRow(toRow(a))).toEqual(a)
  })
})
