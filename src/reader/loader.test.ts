import { describe, expect, it } from 'vitest'
import { bookMediaUrl, directMediaUrl } from './loader'

describe('§6.4 book media served directly', () => {
  it('encodes the book and entry path into a book-scheme URL', () => {
    expect(bookMediaUrl('b1', 'OEBPS/images/a b#1.png')).toBe(
      'linen-book://localhost/b1/OEBPS/images/a%20b%231.png',
    )
  })

  it('serves images, audio and video directly', () => {
    expect(directMediaUrl('b', 'OEBPS/plate-1.png', 'image/png')).toMatch(/^linen-book:/)
    expect(directMediaUrl('b', 'a/s.mp3', 'audio/mpeg')).toMatch(/^linen-book:/)
  })

  it('keeps documents, styles, SVG and fonts on the sanitised or decoded path', () => {
    for (const [name, type] of [
      ['c.xhtml', 'application/xhtml+xml'],
      ['s.css', 'text/css'],
      ['i.svg', 'image/svg+xml'],
      ['f.otf', 'font/otf'],
      // A manifest that lies about the type does not get the direct path.
      ['c.xhtml', 'image/png'],
    ])
      expect(directMediaUrl('b', name, type)).toBeNull()
  })
})
