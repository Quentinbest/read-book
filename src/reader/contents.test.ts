// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { buildContents, currentIndex, fileLabel, type ContentsBook } from './contents'

const doc = (html: string) => new DOMParser().parseFromString(html, 'text/html')

function book(toc: ContentsBook['toc'], docs: (string | null)[]): ContentsBook {
  const sections = docs.map((html, i) => ({
    id: `OEBPS/ch${i}.xhtml`,
    createDocument: () =>
      html === null ? Promise.reject(new Error('damaged')) : Promise.resolve(doc(html)),
  }))
  return {
    toc,
    sections,
    resolveHref: (href) => {
      const i = sections.findIndex((s) => s.id === href.split('#')[0])
      return i >= 0 ? { index: i } : null
    },
  }
}

describe('N6 Contents fallback chain', () => {
  it('uses the navigation first, two levels deep, and marks damaged chapters (E3)', async () => {
    const b = book(
      [
        {
          label: ' Part  One ',
          href: 'OEBPS/ch0.xhtml',
          subitems: [
            {
              label: 'I',
              href: 'OEBPS/ch1.xhtml#a',
              subitems: [{ label: 'deep', href: 'OEBPS/ch1.xhtml#b' }],
            },
          ],
        },
        { label: 'II', href: 'OEBPS/ch2.xhtml' },
      ],
      ['<h1>x</h1>', '<p>', '<p>'],
    )
    const c = await buildContents(b, ['OEBPS/ch2.xhtml'])
    expect(c.source).toBe('nav')
    expect(c.items.map((i) => [i.label, i.depth, i.section, i.damaged])).toEqual([
      ['Part One', 0, 0, false],
      ['I', 1, 1, false],
      ['II', 0, 2, true],
    ])
  })

  it('falls back to headings, labelled as generated', async () => {
    const b = book(null, [
      '<h1 id="t">Title page</h1>',
      '<p>no heading</p>',
      '<h2>Chapter Two</h2>',
      null,
    ])
    const c = await buildContents(b, [])
    expect(c.source).toBe('headings')
    expect(c.items.map((i) => [i.label, i.href, i.damaged])).toEqual([
      ['Title page', 'OEBPS/ch0.xhtml#t', false],
      ['Chapter Two', 'OEBPS/ch2.xhtml', false],
      ['Ch3', 'OEBPS/ch3.xhtml', true], // unreadable: listed and marked, not dropped
    ])
  })

  it('falls back to the spine when nothing has a heading', async () => {
    const c = await buildContents(book([], ['<p>a</p>', '<p>b</p>']), [])
    expect(c.source).toBe('spine')
    expect(c.items.map((i) => i.label)).toEqual(['Ch0', 'Ch1'])
  })

  it('names files readably', () => {
    expect(fileLabel('OEBPS/text/chapter-03_intro.xhtml')).toBe('Chapter 03 intro')
  })
})

describe('“You are here”', () => {
  const items = [
    { label: 'A', href: 'a.xhtml', depth: 0, section: 1, damaged: false },
    { label: 'A2', href: 'a.xhtml#s2', depth: 1, section: 1, damaged: false },
    { label: 'B', href: 'b.xhtml', depth: 0, section: 4, damaged: false },
  ]
  it('prefers the entry foliate reports', () => {
    expect(currentIndex(items, { tocHref: 'a.xhtml#s2', sectionIndex: 1 })).toBe(1)
  })
  it('otherwise takes the last entry at or before the chapter', () => {
    expect(currentIndex(items, { sectionIndex: 3 })).toBe(1)
    expect(currentIndex(items, { sectionIndex: 0 })).toBe(-1)
    expect(currentIndex(items, { sectionIndex: 9 })).toBe(2)
  })
})
