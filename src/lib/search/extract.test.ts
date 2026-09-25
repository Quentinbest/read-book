// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { extractText, rangeFor } from './extract'
import { indexChapter, parseQuery, searchChapter } from './search'

const doc = (body: string) =>
  new DOMParser().parseFromString(`<html><body>${body}</body></html>`, 'text/html')

describe('extractText', () => {
  it('separates block elements and skips ruby annotations and scripts', () => {
    const d = doc(
      '<p>Il était revenu.</p><p>Eh bien<br>voilà</p><p><ruby>智<rt>ち</rt></ruby>に<ruby>働<rp>(</rp><rt>はたら</rt><rp>)</rp></ruby>けば</p><script>x()</script>',
    )
    expect(extractText(d.body).text).toBe('Il était revenu.\nEh bien\nvoilà\n智に働けば\n')
  })

  it('keeps inline elements in the flow', () => {
    expect(extractText(doc('<p>Call <em>me</em> Ish<span>mael</span>.</p>').body).text).toBe(
      'Call me Ishmael.\n',
    )
  })

  it('maps a search match back to a DOM range', () => {
    const d = doc('<p>Some years ago</p><p>Call <em>me Ish</em>mael.</p>')
    const extracted = extractText(d.body)
    const { matches } = searchChapter(
      indexChapter({ index: 0, text: extracted.text }),
      parseQuery('me ishmael')!,
    )
    expect(matches).toHaveLength(1)
    const range = rangeFor(extracted, matches[0].start, matches[0].end)!
    expect(range.toString()).toBe('me Ishmael')
    expect(range.startContainer.parentElement!.localName).toBe('em')
  })
})

describe('offsetAt (A9: selection to quote)', () => {
  it('maps DOM boundary points back to extracted offsets, the inverse of rangeFor', async () => {
    const { extractText, offsetAt, rangeFor } = await import('./extract')
    const doc = new DOMParser().parseFromString(
      '<body><p>Call <em>me</em> Ishmael.</p><p>Some years ago.</p></body>',
      'text/html',
    )
    const x = extractText(doc.body)
    const start = x.text.indexOf('me Ish')
    const end = start + 'me Ish'.length
    const r = rangeFor(x, start, end)!
    expect(offsetAt(x, r.startContainer, r.startOffset)).toBe(start)
    expect(offsetAt(x, r.endContainer, r.endOffset)).toBe(end)
    // An element boundary: before the second paragraph.
    const p2 = doc.querySelectorAll('p')[1]
    expect(offsetAt(x, doc.body, Array.from(doc.body.childNodes).indexOf(p2))).toBe(
      x.text.indexOf('Some'),
    )
  })
})
