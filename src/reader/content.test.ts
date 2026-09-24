import { describe, expect, it } from 'vitest'
import { injectDocumentCsp, sanitizeCss, sanitizeDocumentStyles, transformContent } from './content'

describe('L14 CSS sanitiser', () => {
  it('neutralises fixed and sticky positioning', () => {
    expect(sanitizeCss('div{position:fixed;top:0} h1{position: sticky}')).toBe(
      'div{position: static;top:0} h1{position: static}',
    )
  })

  it('removes remote URLs and imports', () => {
    const css =
      '@import url(http://evil/x.css); @import "https://evil/y.css"; p{background:url("http://evil/bg.png")} q{background:url(img/ok.png)}'
    const out = sanitizeCss(css)
    expect(out).not.toMatch(/evil/)
    expect(out).toContain('url(img/ok.png)')
  })

  it('L13: converts absolute font sizes to rem', () => {
    expect(sanitizeCss('p{font-size:12px} h1{font-size: 18pt} small{font-size:.8em}')).toBe(
      'p{font-size: 0.75rem} h1{font-size: 1.5rem} small{font-size:.8em}',
    )
  })
})

describe('documents', () => {
  it('sanitises <style> blocks and style attributes', () => {
    const html =
      '<html><head><style>div{position:fixed}</style></head><body><p style="position:fixed;font-size:32px">x</p></body></html>'
    const out = sanitizeDocumentStyles(html)
    expect(out).not.toMatch(/fixed/)
    expect(out).toContain('font-size: 2rem')
  })

  it('injects the per-document CSP (D-E1)', () => {
    expect(injectDocumentCsp('<html><head><title>t</title></head></html>')).toMatch(
      /<head><meta http-equiv="Content-Security-Policy" content="default-src 'none'/,
    )
    expect(injectDocumentCsp('<html><body/></html>')).toMatch(/<html><head><meta/)
  })

  it('routes by media type and leaves binary data alone', () => {
    const blob = new Uint8Array([1])
    expect(transformContent(blob, 'image/png')).toBe(blob)
    expect(transformContent('p{position:fixed}', 'text/css')).toBe('p{position: static}')
    expect(transformContent('<html><head></head></html>', 'application/xhtml+xml')).toContain(
      'Content-Security-Policy',
    )
  })
})
