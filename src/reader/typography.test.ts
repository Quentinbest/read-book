import { describe, expect, it } from 'vitest'
import { THEMES } from '../lib/theme/tokens'
import { markBookStyles, transformContent } from './content'
import { computeLayout } from './layout'
import { readerStyles } from './styles'
import {
  PAGE_WIDTH_CH,
  effectivePublisherStyles,
  parseFont,
  parsePageWidth,
  parsePublisherStyles,
} from './typography'

const style = (o: Partial<Parameters<typeof readerStyles>[0]> = {}) =>
  readerStyles({
    fontPx: 19,
    lineHeight: 1.55,
    theme: THEMES.paper,
    hyphenate: false,
    pageHeight: 600,
    ...o,
  })

describe('1.1 settings parse to known values', () => {
  it('falls back to the MVP behaviour', () => {
    expect(parseFont(null)).toBe('book')
    expect(parseFont('comic')).toBe('book')
    expect(parseFont('dyslexic')).toBe('dyslexic')
    expect(parsePageWidth(undefined)).toBe('normal')
    expect(parsePublisherStyles('')).toBe('balanced')
    expect(parsePublisherStyles('off')).toBe('off')
  })

  it('C5: a simplified book is Off whatever the global setting', () => {
    expect(effectivePublisherStyles('full', true)).toBe('off')
    expect(effectivePublisherStyles('full', false)).toBe('full')
  })
})

describe('1.1 font family', () => {
  it('leaves the book its fonts by default', () => {
    expect(style()).not.toMatch(/font-family: [^;]*!important/)
    expect(style({ font: 'book' })).not.toMatch(/font-family: [^;]*!important/)
  })

  it('forces the chosen family over the text, not over code', () => {
    const css = style({ font: 'dyslexic' })
    expect(css).toMatch(/body, body \*:not\(pre\)[^{]*:not\(code \*\) \{ font-family: OpenDyslexic/)
    expect(style({ font: 'sans' })).toMatch(/font-family: -apple-system[^;]*!important/)
  })
})

describe('1.1 page width (L1)', () => {
  const at = (measureCh?: number) =>
    computeLayout({ width: 1600, height: 900, fontPx: 19, spacing: 'default', measureCh })

  it('narrow, normal and wide are L1’s clamp and default', () => {
    expect(Math.round(at(PAGE_WIDTH_CH.narrow).measureCh)).toBe(56)
    expect(Math.round(at(PAGE_WIDTH_CH.normal).measureCh)).toBe(66)
    expect(Math.round(at(PAGE_WIDTH_CH.wide).measureCh)).toBe(74)
    expect(at().columnWidth).toBe(at(66).columnWidth)
  })

  it('never leaves L1’s clamp', () => {
    expect(Math.round(at(200).measureCh)).toBe(74)
    expect(Math.round(at(10).measureCh)).toBe(56)
  })

  it('L5: a wide page gets the long-line bonus', () => {
    expect(at(PAGE_WIDTH_CH.wide).lineHeight).toBeGreaterThan(at(PAGE_WIDTH_CH.normal).lineHeight)
  })
})

describe('C5 publisher styles', () => {
  it('Balanced gives the reader line heights and alignment; Full keeps the publisher’s', () => {
    expect(style({ publisher: 'balanced' })).toMatch(/line-height: inherit !important/)
    expect(style({ publisher: 'balanced' })).toMatch(
      /p, li, dd, blockquote { text-align: start !important/,
    )
    const full = style({ publisher: 'full' })
    expect(full).not.toMatch(/line-height: inherit !important/)
    expect(full).not.toMatch(/p, li, dd, blockquote { text-align: start !important/)
    // Colours stay the reader's in every mode (L14 contrast).
    expect(full).toMatch(/color: inherit !important/)
  })

  it('marks the book’s own stylesheets so Off can turn them off', () => {
    expect(
      markBookStyles('<head><link rel="stylesheet" href="a.css"/><style>p{}</style></head>'),
    ).toBe(
      '<head><link data-linen-book="" rel="stylesheet" href="a.css"/><style data-linen-book="">p{}</style></head>',
    )
    expect(markBookStyles('<styles/><linked/>')).toBe('<styles/><linked/>')
    const doc = transformContent(
      '<html><head><style>p{}</style></head></html>',
      'application/xhtml+xml',
    )
    expect(doc).toMatch(/<style data-linen-book="">/)
  })
})
